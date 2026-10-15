//! Cold UI/owner practice mailbox. Never used by the audio callback.
use beatkernel::{
    audio::{
        PracticeBoundaryKind, PracticeRateEvent, PracticeRateOutcome, PracticeRateRefusal,
        PracticeRestartEvent,
    },
    time::Timestamp,
    transport::Rate,
};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PracticeRateState {
    pub revision: u64,
    pub rate: Rate,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PracticeTransactionCapability {
    pub practice: PracticeCapability,
    pub rate: PracticeRateState,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PracticeTransactionRequest {
    Region(PracticeRequest),
    Rate {
        id: u64,
        generation: u64,
        expected: PracticeRateState,
        requested_rate: Rate,
    },
    RestartOriginal {
        id: u64,
        generation: u64,
        expected: PracticeRateState,
    },
}
impl PracticeTransactionRequest {
    fn identity(self) -> (u64, u64) {
        match self {
            Self::Region(request) => (request.id, request.generation),
            Self::Rate { id, generation, .. } | Self::RestartOriginal { id, generation, .. } => {
                (id, generation)
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PracticeTransactionReply {
    Region {
        reply: PracticeReply,
        rate: Option<PracticeRateState>,
    },
    Rate {
        id: u64,
        /// Generation in the original request; actual cut facts are in result.
        generation: u64,
        result: Result<PracticeRateEvent, String>,
    },
    RestartOriginal {
        id: u64,
        generation: u64,
        result: Result<PracticeRestartEvent, String>,
    },
}
impl PracticeTransactionReply {
    fn identity(&self) -> (u64, u64) {
        match self {
            Self::Region { reply, .. } => (reply.id, reply.generation),
            Self::Rate { id, generation, .. } | Self::RestartOriginal { id, generation, .. } => {
                (*id, *generation)
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PracticeTransactionResponse {
    pub request: PracticeTransactionRequest,
    pub reply: PracticeTransactionReply,
}

#[derive(Clone, Copy)]
enum Capability {
    Legacy(PracticeCapability),
    Transactions(PracticeTransactionCapability),
}
impl Capability {
    fn practice(self) -> PracticeCapability {
        match self {
            Self::Legacy(capability) => capability,
            Self::Transactions(capability) => capability.practice,
        }
    }
    fn transactions(self) -> Option<PracticeTransactionCapability> {
        match self {
            Self::Legacy(_) => None,
            Self::Transactions(capability) => Some(capability),
        }
    }
}
#[derive(Clone, Copy)]
enum Pending {
    Legacy(PracticeRequest),
    Transaction {
        request: PracticeTransactionRequest,
        /// Region's public request retains its original shape.
        region_rate: Option<PracticeRateState>,
    },
}
impl Pending {
    fn request(self) -> PracticeTransactionRequest {
        match self {
            Self::Legacy(request) => PracticeTransactionRequest::Region(request),
            Self::Transaction { request, .. } => request,
        }
    }
}
struct RateOutcomeState {
    generation: u64,
    revision: u64,
    previous: Rate,
    requested: Rate,
    applied: Rate,
    outcome: PracticeRateOutcome,
}
#[derive(Default)]
pub(crate) struct PracticeControls {
    capability: Option<Capability>,
    next_id: Option<u64>,
    pending: Option<Pending>,
    taken: bool,
    reply: Option<PracticeTransactionReply>,
}
impl PracticeControls {
    pub fn new() -> Self {
        Self {
            next_id: Some(1),
            ..Self::default()
        }
    }
    pub fn capability(&self) -> Option<PracticeCapability> {
        self.capability.map(Capability::practice)
    }
    pub fn transaction_capability(&self) -> Option<PracticeTransactionCapability> {
        self.capability.and_then(Capability::transactions)
    }
    pub fn pending_request(&self) -> Option<PracticeRequest> {
        match self.pending? {
            Pending::Legacy(request) => Some(request),
            Pending::Transaction { .. } => None,
        }
    }
    pub fn pending_transaction(&self) -> Option<PracticeTransactionRequest> {
        match self.pending? {
            Pending::Transaction { request, .. } => Some(request),
            Pending::Legacy(_) => None,
        }
    }
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn advertise(&mut self, capability: Option<PracticeCapability>) -> Result<(), String> {
        self.advertise_capability(capability.map(Capability::Legacy))
    }
    pub fn advertise_transactions(
        &mut self,
        capability: Option<PracticeTransactionCapability>,
    ) -> Result<(), String> {
        if capability.is_some_and(|capability| capability.rate.rate.numerator() <= 0) {
            return Err("practice rate must be positive".into());
        }
        self.advertise_capability(capability.map(Capability::Transactions))
    }
    fn advertise_capability(&mut self, capability: Option<Capability>) -> Result<(), String> {
        if let Some(cap) = capability {
            let practice = cap.practice();
            if practice.min_target.as_nanos() < 0 || practice.max_target < practice.min_target {
                return Err("invalid practice capability range".into());
            }
            if self
                .capability()
                .is_some_and(|old| practice.generation < old.generation)
            {
                return Err("stale practice capability".into());
            }
            if self.pending() {
                let old = self.capability.ok_or("practice capability was revoked")?;
                if old.transactions().is_some() != cap.transactions().is_some() {
                    return Err(
                        "practice bridge cannot change while acknowledgement is pending".into(),
                    );
                }
                let old = old.practice();
                if old.min_target != practice.min_target || old.max_target != practice.max_target {
                    return Err(
                        "practice range cannot change while acknowledgement is pending".into(),
                    );
                }
            }
            if let (Some(old), Some(new)) = (self.transaction_capability(), cap.transactions()) {
                if new.rate.revision < old.rate.revision
                    || new.rate.revision == old.rate.revision && new.rate.rate != old.rate.rate
                {
                    return Err("stale practice rate capability".into());
                }
            }
        } else if self.pending() {
            self.revoke("practice capability revoked");
        }
        self.capability = capability;
        Ok(())
    }
    pub fn request(&mut self, action: PracticeAction) -> Result<u64, String> {
        let cap = self
            .capability()
            .ok_or("retained practice is unsupported")?;
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
        let id = self.allocate_id()?;
        let request = PracticeRequest {
            id,
            generation: cap.generation,
            action,
        };
        self.pending = Some(match self.transaction_capability() {
            Some(capability) => Pending::Transaction {
                request: PracticeTransactionRequest::Region(request),
                region_rate: Some(capability.rate),
            },
            None => Pending::Legacy(request),
        });
        self.taken = false;
        Ok(id)
    }
    fn allocate_id(&mut self) -> Result<u64, String> {
        let id = self.next_id.ok_or("practice request identity exhausted")?;
        self.next_id = id.checked_add(1);
        Ok(id)
    }
    pub fn request_rate(&mut self, requested_rate: Rate) -> Result<u64, String> {
        if requested_rate.numerator() <= 0 {
            return Err("practice rate must be positive".into());
        }
        self.request_transaction(Some(requested_rate))
    }
    pub fn request_original_restart(&mut self) -> Result<u64, String> {
        self.request_transaction(None)
    }
    fn request_transaction(&mut self, requested_rate: Option<Rate>) -> Result<u64, String> {
        let capability = self
            .transaction_capability()
            .ok_or("runtime practice is unsupported")?;
        if self.pending() {
            return Err("practice request is pending".into());
        }
        let id = self.allocate_id()?;
        let generation = capability.practice.generation;
        let expected = capability.rate;
        self.pending = Some(Pending::Transaction {
            request: match requested_rate {
                Some(requested_rate) => PracticeTransactionRequest::Rate {
                    id,
                    generation,
                    expected,
                    requested_rate,
                },
                None => PracticeTransactionRequest::RestartOriginal {
                    id,
                    generation,
                    expected,
                },
            },
            region_rate: None,
        });
        self.taken = false;
        Ok(id)
    }
    pub fn rate_pending(&self) -> bool {
        matches!(
            self.pending_transaction(),
            Some(PracticeTransactionRequest::Rate { .. })
        )
    }
    pub fn take_request(&mut self) -> Option<PracticeRequest> {
        if self.taken || self.reply.is_some() {
            return None;
        }
        let request = self.pending_request()?;
        self.taken = true;
        Some(request)
    }
    pub fn take_transaction(&mut self, active: bool) -> Option<PracticeTransactionRequest> {
        if self.taken || self.reply.is_some() {
            return None;
        }
        let request = self.pending_transaction()?;
        if !active && matches!(request, PracticeTransactionRequest::Rate { .. }) {
            return None;
        }
        self.taken = true;
        Some(request)
    }
    pub fn reply(&mut self, reply: &PracticeReply) -> Result<(), String> {
        let pending = self.pending.ok_or("no practice request is pending")?;
        let Pending::Legacy(request) = pending else {
            return Err("typed practice acknowledgement is required".into());
        };
        if !self.taken
            || self.reply.is_some()
            || reply.id != request.id
            || reply.generation != request.generation
        {
            return Err("stale practice acknowledgement".into());
        }
        let capability = self.validate_region_reply(request, reply)?;
        if let Some(capability) = capability {
            self.capability = Some(Capability::Legacy(capability));
        }
        self.reply = Some(PracticeTransactionReply::Region {
            reply: reply.clone(),
            rate: None,
        });
        Ok(())
    }
    fn validate_region_reply(
        &self,
        request: PracticeRequest,
        reply: &PracticeReply,
    ) -> Result<Option<PracticeCapability>, String> {
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
            let cap = self.capability().ok_or("practice capability was revoked")?;
            if applied.generation < cap.generation {
                return Err("practice acknowledgement precedes current generation".into());
            }
            if target.is_some_and(|target| target != applied.requested_target)
                || applied.applied_target < cap.min_target
                || applied.applied_target > cap.max_target
            {
                return Err("practice acknowledgement target mismatch".into());
            }
            return Ok(Some(PracticeCapability {
                generation: applied.generation,
                ..cap
            }));
        }
        Ok(None)
    }
    pub fn take_response(&mut self) -> Option<PracticeResponse> {
        let request = self.pending_request()?;
        let PracticeTransactionReply::Region { reply, .. } = self.reply.take()? else {
            unreachable!("legacy requests retain region replies")
        };
        self.pending = None;
        self.taken = false;
        Some(PracticeResponse { request, reply })
    }
    pub fn reply_transaction(&mut self, reply: &PracticeTransactionReply) -> Result<(), String> {
        let pending = self.pending.ok_or("no practice request is pending")?;
        let Pending::Transaction {
            request,
            region_rate,
        } = pending
        else {
            return Err("legacy practice acknowledgement is required".into());
        };
        if !self.taken || self.reply.is_some() || reply.identity() != request.identity() {
            return Err("stale practice acknowledgement".into());
        }
        let mut capability = self
            .transaction_capability()
            .ok_or("practice capability was revoked")?;
        match (request, reply) {
            (
                PracticeTransactionRequest::Region(request),
                PracticeTransactionReply::Region { reply, rate },
            ) => {
                let updated = self.validate_region_reply(request, reply)?;
                if let Some(updated) = updated {
                    let expected =
                        region_rate.ok_or("practice region rate state is unavailable")?;
                    let rate = rate.ok_or("practice region acknowledgement lacks rate state")?;
                    let generation = if request.action == PracticeAction::DisableLoop {
                        capability.practice.generation
                    } else {
                        capability
                            .practice
                            .generation
                            .checked_add(1)
                            .ok_or("practice generation exhausted")?
                    };
                    if capability.rate != expected
                        || rate.rate != expected.rate
                        || expected.revision.checked_add(1) != Some(rate.revision)
                        || updated.generation != generation
                    {
                        return Err("invalid practice region rate acknowledgement".into());
                    }
                    capability = PracticeTransactionCapability {
                        practice: updated,
                        rate,
                    };
                } else if rate.is_some_and(|rate| rate != capability.rate) {
                    return Err("practice refusal changed rate state".into());
                }
            }
            (
                PracticeTransactionRequest::Rate {
                    id,
                    generation,
                    expected,
                    requested_rate,
                },
                PracticeTransactionReply::Rate { result, .. },
            ) => {
                if let Ok(event) = result {
                    if event.request_id != id
                        || event.request_revision != expected.revision
                        || event.requested_rate != requested_rate
                        || event.physical_frame < event.playback_frame
                        || event.active_elapsed_nanos < 0
                    {
                        return Err("invalid practice rate acknowledgement".into());
                    }
                    capability = Self::validate_rate_outcome(
                        capability,
                        generation,
                        expected,
                        RateOutcomeState {
                            generation: event.generation,
                            revision: event.applied_revision,
                            previous: event.previous_rate,
                            requested: event.requested_rate,
                            applied: event.applied_rate,
                            outcome: event.outcome,
                        },
                        false,
                    )?;
                }
            }
            (
                PracticeTransactionRequest::RestartOriginal {
                    id,
                    generation,
                    expected,
                },
                PracticeTransactionReply::RestartOriginal { result, .. },
            ) => {
                if let Ok(event) = result {
                    let boundary = event.boundary;
                    if boundary.request_id != id
                        || event.request_revision != expected.revision
                        || boundary.physical_frame < boundary.playback_frame
                        || boundary.requested_song_time != boundary.applied_song_time
                        || boundary.correction_nanos != 0
                        || match event.outcome {
                            PracticeRateOutcome::Applied => {
                                boundary.kind != PracticeBoundaryKind::Requested
                                    || boundary.iteration != 0
                                    || boundary.applied_song_time < capability.practice.min_target
                                    || boundary.applied_song_time > capability.practice.max_target
                            }
                            PracticeRateOutcome::Refused(_) => {
                                boundary.kind != PracticeBoundaryKind::ControlRejectedRevision
                            }
                            PracticeRateOutcome::Noop => true,
                        }
                    {
                        return Err("invalid original restart acknowledgement".into());
                    }
                    capability = Self::validate_rate_outcome(
                        capability,
                        generation,
                        expected,
                        RateOutcomeState {
                            generation: boundary.generation,
                            revision: event.applied_revision,
                            previous: event.previous_rate,
                            requested: event.requested_rate,
                            applied: event.applied_rate,
                            outcome: event.outcome,
                        },
                        true,
                    )?;
                }
            }
            _ => return Err("practice acknowledgement variant mismatch".into()),
        }
        self.capability = Some(Capability::Transactions(capability));
        self.reply = Some(reply.clone());
        Ok(())
    }
    fn validate_rate_outcome(
        mut capability: PracticeTransactionCapability,
        generation: u64,
        expected: PracticeRateState,
        state: RateOutcomeState,
        restart: bool,
    ) -> Result<PracticeTransactionCapability, String> {
        let RateOutcomeState {
            generation: actual_generation,
            revision: applied_revision,
            previous: previous_rate,
            requested: requested_rate,
            applied: applied_rate,
            outcome,
        } = state;
        if previous_rate.numerator() <= 0
            || requested_rate.numerator() <= 0
            || applied_rate.numerator() <= 0
        {
            return Err("practice rate acknowledgement must be positive".into());
        }
        match outcome {
            PracticeRateOutcome::Applied | PracticeRateOutcome::Noop => {
                let next_generation = if restart {
                    generation.checked_add(1)
                } else {
                    Some(generation)
                };
                if capability.practice.generation != generation
                    || capability.rate != expected
                    || next_generation != Some(actual_generation)
                    || expected.revision.checked_add(1) != Some(applied_revision)
                    || previous_rate != expected.rate
                    || applied_rate != requested_rate
                    || match outcome {
                        PracticeRateOutcome::Noop => restart || requested_rate != previous_rate,
                        PracticeRateOutcome::Applied => !restart && requested_rate == previous_rate,
                        PracticeRateOutcome::Refused(_) => unreachable!(),
                    }
                {
                    return Err("invalid applied practice rate state".into());
                }
                capability.practice.generation = actual_generation;
                capability.rate = PracticeRateState {
                    revision: applied_revision,
                    rate: applied_rate,
                };
            }
            PracticeRateOutcome::Refused(refusal) => {
                if actual_generation != capability.practice.generation
                    || applied_revision != capability.rate.revision
                    || previous_rate != capability.rate.rate
                    || applied_rate != previous_rate
                    || match refusal {
                        PracticeRateRefusal::StaleGeneration => {
                            actual_generation <= generation || applied_revision < expected.revision
                        }
                        PracticeRateRefusal::StaleRevision => {
                            actual_generation != generation || applied_revision <= expected.revision
                        }
                        _ => actual_generation != generation || capability.rate != expected,
                    }
                {
                    return Err("invalid refused practice rate state".into());
                }
            }
        }
        Ok(capability)
    }
    pub fn take_transaction_response(&mut self) -> Option<PracticeTransactionResponse> {
        let request = self.pending_transaction()?;
        let reply = self.reply.take()?;
        self.pending = None;
        self.taken = false;
        Some(PracticeTransactionResponse { request, reply })
    }
    pub fn revoke(&mut self, reason: &str) {
        self.capability = None;
        if self.reply.is_none() {
            if let Some(request) = self.pending {
                self.reply = Some(match request.request() {
                    PracticeTransactionRequest::Region(request) => {
                        PracticeTransactionReply::Region {
                            reply: PracticeReply {
                                id: request.id,
                                generation: request.generation,
                                result: Err(reason.into()),
                            },
                            rate: None,
                        }
                    }
                    PracticeTransactionRequest::Rate { id, generation, .. } => {
                        PracticeTransactionReply::Rate {
                            id,
                            generation,
                            result: Err(reason.into()),
                        }
                    }
                    PracticeTransactionRequest::RestartOriginal { id, generation, .. } => {
                        PracticeTransactionReply::RestartOriginal {
                            id,
                            generation,
                            result: Err(reason.into()),
                        }
                    }
                });
            }
        }
    }
}
