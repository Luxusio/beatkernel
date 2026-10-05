use std::{
    cmp::Reverse,
    collections::{BTreeSet, BinaryHeap, HashMap, HashSet},
};

use crate::{
    chart::{CompiledChart, ObjectId},
    input::{
        ButtonState, ContactId, EventMeta, GameControlId, GameInputEvent, PhysicalInputEvent,
        TouchPhase,
    },
    interaction::{
        ActiveInteraction, BeginContext, InputOwner, InteractionContext, InteractionOutput,
        InteractionState, StartEligibility,
    },
    time::Timestamp,
};

use super::{
    hazard::HazardState, Candidate, CandidateResolver, ClosestCandidate, HazardError, HazardEvent,
    HazardTimeline, JudgeError, JudgeEvent, JudgePolicy, JudgeProfile, Rule, WindowJudgePolicy,
};

/// Single-owner forward judge with indexed starts and ordered deadlines.
///
/// Setup and result vectors can allocate; this API is intended for a gameplay
/// thread. Inputs retain their original metadata and take explicit song time.
pub struct JudgeEngine {
    chart: CompiledChart,
    profile: JudgeProfile,
    resolver: Box<dyn CandidateResolver>,
    policy: Box<dyn JudgePolicy>,
    interactions: Vec<Box<dyn ActiveInteraction>>,
    controls: Vec<GameControlId>,
    eligibility: Vec<StartEligibility>,
    starts: HashMap<GameControlId, Vec<usize>>,
    custom_pending: HashMap<GameControlId, BTreeSet<(ObjectId, usize)>>,
    identities: HashMap<ObjectId, usize>,
    deadlines: BinaryHeap<Reverse<(i128, ObjectId, usize)>>,
    scheduled: Vec<Option<i128>>,
    active: BTreeSet<(ObjectId, usize)>,
    active_controls: HashMap<GameControlId, BTreeSet<(ObjectId, usize)>>,
    held: HashSet<InputOwner>,
    held_contacts: HashSet<(InputOwner, ContactId)>,
    contact_enabled: bool,
    hazards: Option<HazardState>,
    effective_time: Option<Timestamp>,
    initial_configuration: Result<Vec<u8>, SnapshotError>,
}

impl JudgeEngine {
    /// Validates all registrations and objects before beginning interactions.
    pub fn new(
        chart: CompiledChart,
        rules: Vec<Rule>,
        profile: JudgeProfile,
    ) -> Result<Self, JudgeError> {
        Self::with_policies(
            chart,
            rules,
            profile,
            Box::new(ClosestCandidate),
            Box::new(WindowJudgePolicy),
        )
    }

    /// Enables contact ownership even when no chart object uses press semantics.
    /// Tracking does not change object eligibility or create judged objects.
    pub fn new_with_contacts(
        chart: CompiledChart,
        rules: Vec<Rule>,
        profile: JudgeProfile,
    ) -> Result<Self, JudgeError> {
        Self::with_policies_and_contacts(
            chart,
            rules,
            profile,
            Box::new(ClosestCandidate),
            Box::new(WindowJudgePolicy),
        )
    }

    /// Constructs a judge with caller-selected candidate and grading policies.
    pub fn with_policies(
        chart: CompiledChart,
        rules: Vec<Rule>,
        profile: JudgeProfile,
        resolver: Box<dyn CandidateResolver>,
        policy: Box<dyn JudgePolicy>,
    ) -> Result<Self, JudgeError> {
        Self::build(chart, rules, profile, resolver, policy, false)
    }

    /// Uses caller policies with immutable contact ownership enabled, including
    /// empty charts. Candidate eligibility remains defined by each evaluator.
    pub fn with_policies_and_contacts(
        chart: CompiledChart,
        rules: Vec<Rule>,
        profile: JudgeProfile,
        resolver: Box<dyn CandidateResolver>,
        policy: Box<dyn JudgePolicy>,
    ) -> Result<Self, JudgeError> {
        Self::build(chart, rules, profile, resolver, policy, true)
    }

    fn build(
        chart: CompiledChart,
        rules: Vec<Rule>,
        profile: JudgeProfile,
        resolver: Box<dyn CandidateResolver>,
        policy: Box<dyn JudgePolicy>,
        contacts: bool,
    ) -> Result<Self, JudgeError> {
        let mut registrations = HashMap::new();
        for rule in &rules {
            if registrations.insert(rule.interaction, rule).is_some() {
                return Err(JudgeError::DuplicateRule {
                    id: rule.interaction,
                });
            }
        }
        // Complete fallible validation before invoking even the first begin.
        for object in chart.objects() {
            let rule =
                registrations
                    .get(&object.interaction)
                    .ok_or(JudgeError::UnknownInteraction {
                        id: object.interaction,
                    })?;
            rule.evaluator.validate(object, &profile)?;
        }
        let mut interactions = Vec::with_capacity(chart.objects().len());
        let mut controls = Vec::with_capacity(chart.objects().len());
        let mut eligibility = Vec::with_capacity(chart.objects().len());
        let mut starts: HashMap<GameControlId, Vec<usize>> = HashMap::new();
        let mut identities = HashMap::new();
        for (index, object) in chart.objects().iter().enumerate() {
            let rule = registrations[&object.interaction];
            interactions.push(rule.evaluator.begin(
                object,
                &BeginContext {
                    control: rule.control,
                    profile: &profile,
                },
            ));
            controls.push(rule.control);
            let start_eligibility = rule.evaluator.start_eligibility();
            eligibility.push(start_eligibility);
            if matches!(
                start_eligibility,
                StartEligibility::ProfileButtonPress | StartEligibility::ProfilePress
            ) {
                starts.entry(rule.control).or_default().push(index);
            }
            identities.insert(object.id, index);
        }
        let count = interactions.len();
        let contact_enabled = contacts || eligibility.contains(&StartEligibility::ProfilePress);
        let mut engine = Self {
            chart,
            profile,
            resolver,
            policy,
            interactions,
            controls,
            eligibility,
            starts,
            custom_pending: HashMap::new(),
            identities,
            deadlines: BinaryHeap::new(),
            scheduled: vec![None; count],
            active: BTreeSet::new(),
            active_controls: HashMap::new(),
            held: HashSet::new(),
            held_contacts: HashSet::new(),
            contact_enabled,
            hazards: None,
            effective_time: None,
            initial_configuration: Err(SnapshotError::ConfigurationMismatch),
        };
        for index in 0..count {
            engine.refresh(index, None);
        }
        engine.initial_configuration = engine.canonical_state_bytes();
        Ok(engine)
    }

    /// Installs one immutable hazard timeline before any accepted operation.
    /// Snapshot support is checked before committing the new configuration.
    pub fn configure_hazards(&mut self, timeline: HazardTimeline) -> Result<(), HazardError> {
        if self.hazards.is_some() {
            return Err(HazardError::AlreadyConfigured);
        }
        if self.effective_time.is_some() {
            return Err(HazardError::AlreadyStarted);
        }
        let hazards = HazardState::new(timeline)?;
        let configuration = self
            .canonical_state_bytes_with_hazards(Some(&hazards))
            .map_err(HazardError::Snapshot)?;
        self.hazards = Some(hazards);
        self.initial_configuration = Ok(configuration);
        Ok(())
    }

    /// Borrows the last successful input/advance's hazard report. Rejected
    /// operations preserve it; unconfigured engines always report an empty slice.
    pub fn hazard_events(&self) -> &[HazardEvent] {
        self.hazards.as_ref().map_or(&[], HazardState::events)
    }

    /// Checks actual button/contact ownership without committing a press.
    /// Only a new Down is fresh; touch also requires enabled contact semantics.
    pub fn is_fresh_press(&self, event: &GameInputEvent) -> bool {
        match &event.physical {
            PhysicalInputEvent::Button(button) => {
                button.state == ButtonState::Down
                    && !self.held.contains(&InputOwner {
                        source: button.meta.source,
                        physical: button.control,
                        game_control: event.game_control,
                    })
            }
            PhysicalInputEvent::Touch(touch) => {
                self.contact_enabled
                    && touch.phase == TouchPhase::Down
                    && !self.held_contacts.contains(&(
                        InputOwner {
                            source: touch.meta.source,
                            physical: touch.control,
                            game_control: event.game_control,
                        },
                        touch.contact,
                    ))
            }
            _ => false,
        }
    }

    /// Processes unchanged bound input at unoffset mapped song time.
    ///
    /// Time and resolver errors leave engine-owned state unchanged. Equal
    /// effective timestamps are valid, including an inclusive deadline.
    pub fn push_input(
        &mut self,
        event: &GameInputEvent,
        mapped_song_time: Timestamp,
    ) -> Result<Vec<JudgeEvent>, JudgeError> {
        let time = self.checked_time(mapped_song_time)?;
        let button = match &event.physical {
            PhysicalInputEvent::Button(button) => Some((
                InputOwner {
                    source: button.meta.source,
                    physical: button.control,
                    game_control: event.game_control,
                },
                button.state,
            )),
            _ => None,
        };
        let contact = match &event.physical {
            PhysicalInputEvent::Touch(touch) if self.contact_enabled => Some((
                (
                    InputOwner {
                        source: touch.meta.source,
                        physical: touch.control,
                        game_control: event.game_control,
                    },
                    touch.contact,
                ),
                touch.phase,
            )),
            _ => None,
        };
        let fresh = self.is_fresh_press(event);
        let fresh_button = button.is_some() && fresh;
        let fresh_contact = contact.is_some() && fresh;
        let candidates = self.candidates(event, time, fresh_button, fresh_contact);
        let selected = if candidates.is_empty() {
            None
        } else {
            self.resolver.select(&candidates)
        };
        let selected_index = match selected {
            Some(object) => {
                if !candidates
                    .iter()
                    .any(|candidate| candidate.object == object)
                {
                    return Err(JudgeError::InvalidCandidate { object });
                }
                Some(self.identities[&object])
            }
            None => None,
        };

        // No library-owned fallible work remains after this point.
        if let Some(hazards) = &mut self.hazards {
            hazards.clear_events();
            hazards.consume(time, false, None);
        }
        let mut output = Vec::new();
        self.expire(time, &mut output);
        if let Some((owner, state)) = button {
            match state {
                ButtonState::Down => {
                    if self.held.insert(owner) {
                        if let Some(hazards) = &mut self.hazards {
                            hazards.acquired(owner.game_control);
                        }
                    }
                }
                ButtonState::Up => {
                    if self.held.remove(&owner) {
                        if let Some(hazards) = &mut self.hazards {
                            hazards.released(owner.game_control);
                        }
                    }
                }
                ButtonState::Repeat => {}
            }
        }
        if let Some((owner, phase)) = contact {
            match phase {
                TouchPhase::Down => {
                    if self.held_contacts.insert(owner) {
                        if let Some(hazards) = &mut self.hazards {
                            hazards.acquired(owner.0.game_control);
                        }
                    }
                }
                TouchPhase::Up | TouchPhase::Cancel => {
                    if self.held_contacts.remove(&owner) {
                        if let Some(hazards) = &mut self.hazards {
                            hazards.released(owner.0.game_control);
                        }
                    }
                }
                TouchPhase::Move => {}
            }
        }
        if let Some(hazards) = &mut self.hazards {
            hazards.consume(time, true, Some(*event.physical.meta()));
        }
        let mut dispatch: BTreeSet<(ObjectId, usize)> = self
            .active_controls
            .get(&event.game_control)
            .cloned()
            .unwrap_or_default();
        if let Some(index) = selected_index {
            dispatch.insert((self.chart.objects()[index].id, index));
        }
        for (_, index) in dispatch {
            let context = self.context(time);
            if self.interactions[index].state() == InteractionState::Completed
                || !self.interactions[index].accepts_input(event, &context)
            {
                continue;
            }
            let context = InteractionContext {
                song_time: time,
                profile: &self.profile,
                policy: self.policy.as_ref(),
            };
            let results = self.interactions[index].on_input(event, &context);
            self.stamp(
                index,
                time,
                Some(*event.physical.meta()),
                results,
                &mut output,
            );
            self.refresh(index, None);
        }
        self.effective_time = Some(time);
        Ok(output)
    }

    /// Advances at unoffset mapped song time, expiring strictly older deadlines.
    pub fn advance_to(
        &mut self,
        mapped_song_time: Timestamp,
    ) -> Result<Vec<JudgeEvent>, JudgeError> {
        let time = self.checked_time(mapped_song_time)?;
        if let Some(hazards) = &mut self.hazards {
            hazards.clear_events();
            hazards.consume(time, true, None);
        }
        let mut output = Vec::new();
        let advanced = self.expire(time, &mut output);
        let active: Vec<_> = self.active.iter().copied().collect();
        for (_, index) in active {
            if !advanced.contains(&index) {
                self.advance_interaction(index, time, &mut output);
            }
        }
        self.effective_time = Some(time);
        Ok(output)
    }

    /// Inspects a chart-local object's lifecycle, or returns `None` if unknown.
    pub fn state(&self, object: ObjectId) -> Option<InteractionState> {
        self.identities
            .get(&object)
            .map(|&index| self.interactions[index].state())
    }

    /// Returns the last accepted operation's effective time, if any.
    pub const fn effective_song_time(&self) -> Option<Timestamp> {
        self.effective_time
    }

    /// Reports button ownership scoped to one bound logical destination.
    pub fn is_held(&self, owner: InputOwner) -> bool {
        self.held.contains(&owner)
    }

    /// Borrows the immutable compiled chart owned by this judge.
    pub const fn chart(&self) -> &CompiledChart {
        &self.chart
    }

    /// Borrows the immutable timing profile used by inputs and advances.
    pub const fn profile(&self) -> &JudgeProfile {
        &self.profile
    }

    fn checked_time(&self, mapped: Timestamp) -> Result<Timestamp, JudgeError> {
        let time = mapped
            .checked_add(self.profile.input_offset())
            .ok_or(JudgeError::Overflow)?;
        if self.effective_time.is_some_and(|previous| time < previous) {
            return Err(JudgeError::NonMonotonicSongTime);
        }
        Ok(time)
    }

    fn context(&self, time: Timestamp) -> InteractionContext<'_> {
        InteractionContext {
            song_time: time,
            profile: &self.profile,
            policy: self.policy.as_ref(),
        }
    }

    fn candidates(
        &self,
        event: &GameInputEvent,
        time: Timestamp,
        fresh_button: bool,
        fresh_contact: bool,
    ) -> Vec<Candidate> {
        let now = i128::from(time.as_nanos());
        let context = self.context(time);
        let mut candidates = Vec::new();
        let mut consider = |index: usize| {
            let interaction = &self.interactions[index];
            let object = &self.chart.objects()[index];
            if interaction.state() == InteractionState::Pending
                && interaction
                    .deadline(&self.profile)
                    .is_none_or(|deadline| deadline >= now)
                && interaction.accepts_input(event, &context)
            {
                candidates.push(Candidate {
                    object: object.id,
                    target: object.time.start,
                    delta: now - i128::from(object.time.start.as_nanos()),
                });
            }
        };
        if fresh_button || fresh_contact {
            if let Some(starts) = self.starts.get(&event.game_control) {
                let first_time = now - i128::from(self.profile.max_late().as_nanos());
                let last_time = now + i128::from(self.profile.max_early().as_nanos());
                let first = starts.partition_point(|&index| {
                    i128::from(self.chart.objects()[index].time.start.as_nanos()) < first_time
                });
                let last = starts.partition_point(|&index| {
                    i128::from(self.chart.objects()[index].time.start.as_nanos()) <= last_time
                });
                for &index in &starts[first..last] {
                    // A contact start must not widen button-only declarations,
                    // including caller-provided predicates in a mixed chart.
                    if fresh_button || self.eligibility[index] == StartEligibility::ProfilePress {
                        consider(index);
                    }
                }
            }
        }
        if let Some(pending) = self.custom_pending.get(&event.game_control) {
            for &(_, index) in pending {
                consider(index);
            }
        }
        candidates
    }

    fn refresh(&mut self, index: usize, consumed_deadline: Option<i128>) {
        let key = (self.chart.objects()[index].id, index);
        let mut routes = vec![self.controls[index]];
        for control in self.interactions[index].additional_controls() {
            if !routes.contains(control) {
                routes.push(*control);
            }
        }
        if self.eligibility[index] == StartEligibility::EvaluatorDefined {
            for control in &routes {
                if self.interactions[index].state() == InteractionState::Pending {
                    self.custom_pending.entry(*control).or_default().insert(key);
                } else if let Some(indices) = self.custom_pending.get_mut(control) {
                    indices.remove(&key);
                }
            }
        }
        if self.interactions[index].state() == InteractionState::Active {
            self.active.insert(key);
            for control in &routes {
                self.active_controls
                    .entry(*control)
                    .or_default()
                    .insert(key);
            }
        } else {
            self.active.remove(&key);
            for control in &routes {
                if let Some(indices) = self.active_controls.get_mut(control) {
                    indices.remove(&key);
                }
            }
        }
        let deadline = if self.interactions[index].state() == InteractionState::Completed {
            None
        } else {
            self.interactions[index]
                .deadline(&self.profile)
                // A faulty custom callback cannot create an infinite loop by
                // retaining or moving backward a deadline it just consumed.
                .filter(|&next| consumed_deadline.is_none_or(|previous| next > previous))
        };
        if deadline != self.scheduled[index] {
            self.scheduled[index] = deadline;
            if let Some(deadline) = deadline {
                self.deadlines.push(Reverse((deadline, key.0, index)));
            }
        }
    }

    fn expire(&mut self, time: Timestamp, output: &mut Vec<JudgeEvent>) -> HashSet<usize> {
        let mut advanced = HashSet::new();
        while let Some(&Reverse((deadline, _, index))) = self.deadlines.peek() {
            if deadline >= i128::from(time.as_nanos()) {
                break;
            }
            self.deadlines.pop();
            if self.scheduled[index] != Some(deadline)
                || self.interactions[index].state() == InteractionState::Completed
                || self.interactions[index].deadline(&self.profile) != Some(deadline)
            {
                continue;
            }
            advanced.insert(index);
            self.advance_interaction(index, time, output);
        }
        advanced
    }

    fn advance_interaction(&mut self, index: usize, time: Timestamp, output: &mut Vec<JudgeEvent>) {
        let previous = self.scheduled[index];
        let context = InteractionContext {
            song_time: time,
            profile: &self.profile,
            policy: self.policy.as_ref(),
        };
        let results = self.interactions[index].advance_to(time, &context);
        self.stamp(index, time, None, results, output);
        self.refresh(
            index,
            previous.filter(|&deadline| deadline < i128::from(time.as_nanos())),
        );
    }

    fn stamp(
        &self,
        index: usize,
        time: Timestamp,
        input: Option<EventMeta>,
        results: InteractionOutput,
        output: &mut Vec<JudgeEvent>,
    ) {
        output.extend(results.results.into_iter().map(|result| JudgeEvent {
            object: self.chart.objects()[index].id,
            stage: result.stage,
            outcome: result.outcome,
            at: time,
            input,
        }));
    }
}

/// A complete reusable in-memory checkpoint, including custom object state.
/// Allocates off the real-time boundary; it does not snapshot an audio device.
pub struct JudgeSnapshot {
    engine: JudgeEngine,
}

/// Explicit failure to capture or restore a complete checkpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotError {
    /// This object's implementation has not opted into complete checkpoints.
    UnsupportedInteraction {
        /// Chart-local object whose complete state cannot be captured.
        object: ObjectId,
    },
    /// The grading policy has not opted into complete checkpoints.
    UnsupportedPolicy,
    /// The candidate resolver has not opted into complete checkpoints.
    UnsupportedResolver,
    /// Compiled chart, profile or registered routing differs.
    ConfigurationMismatch,
}
impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedInteraction { object } => {
                write!(f, "object {} does not support complete snapshots", object.0)
            }
            Self::UnsupportedPolicy => {
                f.write_str("judge policy does not support complete snapshots")
            }
            Self::UnsupportedResolver => {
                f.write_str("candidate resolver does not support complete snapshots")
            }
            Self::ConfigurationMismatch => {
                f.write_str("snapshot chart/profile/routing does not match engine")
            }
        }
    }
}
impl std::error::Error for SnapshotError {}

impl JudgeSnapshot {
    /// Last accepted effective song time captured by this checkpoint.
    pub fn effective_song_time(&self) -> Option<Timestamp> {
        self.engine.effective_time
    }
    /// Canonical complete-state diagnostic hash with a fixed versioned encoding.
    pub fn stable_hash(&self) -> Result<u64, SnapshotError> {
        self.engine.stable_hash()
    }
}

impl JudgeEngine {
    /// Captures every state affecting judging, failing on unsupported custom state.
    pub fn snapshot(&self) -> Result<JudgeSnapshot, SnapshotError> {
        Ok(JudgeSnapshot {
            engine: self.clone_checkpoint()?,
        })
    }

    /// Builds an independent engine from a reusable checkpoint.
    pub fn from_snapshot(snapshot: &JudgeSnapshot) -> Result<Self, SnapshotError> {
        snapshot.engine.clone_checkpoint()
    }

    /// Atomically restores a compatible checkpoint after every clone succeeds.
    /// The snapshot can be restored repeatedly; callback/external effects remain
    /// the responsibility of custom implementations.
    pub fn restore(&mut self, snapshot: &JudgeSnapshot) -> Result<(), SnapshotError> {
        let source = &snapshot.engine;
        if self.chart != source.chart
            || self.profile != source.profile
            || self.controls != source.controls
            || self.eligibility != source.eligibility
            || self.contact_enabled != source.contact_enabled
            || self.initial_configuration != source.initial_configuration
        {
            return Err(SnapshotError::ConfigurationMismatch);
        }
        let replacement = source.clone_checkpoint()?;
        *self = replacement;
        Ok(())
    }

    /// Hashes complete logical state, not merely time or lifecycle labels.
    pub fn stable_hash(&self) -> Result<u64, SnapshotError> {
        let mut bytes = super::snapshot::Encoder::new(b"beatkernel-judge-complete/v2");
        bytes.bytes(self.initial_configuration.as_ref().map_err(Clone::clone)?);
        bytes.bytes(&self.canonical_state_bytes()?);
        Ok(super::snapshot::hash(&bytes.finish()))
    }

    pub(crate) fn canonical_state_bytes(&self) -> Result<Vec<u8>, SnapshotError> {
        self.canonical_state_bytes_with_hazards(self.hazards.as_ref())
    }

    fn canonical_state_bytes_with_hazards(
        &self,
        hazards: Option<&HazardState>,
    ) -> Result<Vec<u8>, SnapshotError> {
        use super::snapshot::Encoder;
        let mut bytes = Encoder::new(b"beatkernel-judge-state/v1");
        bytes.chart(&self.chart);
        bytes.profile(&self.profile);
        bytes.bytes(
            &self
                .resolver
                .snapshot_bytes()
                .ok_or(SnapshotError::UnsupportedResolver)?,
        );
        bytes.bytes(
            &self
                .policy
                .snapshot_bytes()
                .ok_or(SnapshotError::UnsupportedPolicy)?,
        );
        bytes.u64(self.interactions.len() as u64);
        for (index, interaction) in self.interactions.iter().enumerate() {
            bytes.u64(self.chart.objects()[index].id.0);
            bytes.u32(self.controls[index].0);
            bytes.u64(interaction.additional_controls().len() as u64);
            for control in interaction.additional_controls() {
                bytes.u32(control.0);
            }
            bytes.u8(match self.eligibility[index] {
                StartEligibility::ProfileButtonPress => 0,
                StartEligibility::EvaluatorDefined => 1,
                StartEligibility::ProfilePress => 2,
            });
            bytes.bytes(&interaction.snapshot_bytes().ok_or(
                SnapshotError::UnsupportedInteraction {
                    object: self.chart.objects()[index].id,
                },
            )?);
            bytes.option(self.scheduled[index], Encoder::i128);
        }
        let mut owners: Vec<_> = self
            .held
            .iter()
            .map(|owner| {
                let mut entry = Encoder::new(b"owner/v1");
                entry.owner(*owner);
                entry.finish()
            })
            .collect();
        owners.sort();
        bytes.u64(owners.len() as u64);
        for owner in owners {
            bytes.bytes(&owner);
        }
        bytes.option(self.effective_time, |out, time| out.i64(time.as_nanos()));
        if self.contact_enabled {
            // Append only for opt-in engines: legacy canonical bytes stay exact.
            let mut contacts: Vec<_> = self
                .held_contacts
                .iter()
                .map(|(owner, contact)| {
                    let mut entry = Encoder::new(b"contact-owner/v1");
                    entry.owner(*owner);
                    entry.u64(contact.0);
                    entry.finish()
                })
                .collect();
            contacts.sort();
            let mut extension = Encoder::new(b"judge-held-contacts/v1");
            extension.u64(contacts.len() as u64);
            for contact in contacts {
                extension.bytes(&contact);
            }
            bytes.bytes(&extension.finish());
        }
        if let Some(hazards) = hazards {
            hazards.encode(&mut bytes);
        }
        Ok(bytes.finish())
    }

    fn clone_checkpoint(&self) -> Result<Self, SnapshotError> {
        // Require canonical bytes as well as cloning: unsupported custom states
        // must never enter a purported complete deterministic checkpoint.
        self.initial_configuration.as_ref().map_err(Clone::clone)?;
        self.canonical_state_bytes()?;
        let resolver = self
            .resolver
            .snapshot_clone()
            .ok_or(SnapshotError::UnsupportedResolver)?;
        let policy = self
            .policy
            .snapshot_clone()
            .ok_or(SnapshotError::UnsupportedPolicy)?;
        let interactions = self
            .interactions
            .iter()
            .enumerate()
            .map(|(index, interaction)| {
                interaction
                    .snapshot_clone()
                    .ok_or(SnapshotError::UnsupportedInteraction {
                        object: self.chart.objects()[index].id,
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let clone = Self {
            chart: self.chart.clone(),
            profile: self.profile.clone(),
            resolver,
            policy,
            interactions,
            controls: self.controls.clone(),
            eligibility: self.eligibility.clone(),
            starts: self.starts.clone(),
            custom_pending: self.custom_pending.clone(),
            identities: self.identities.clone(),
            // Preserve consumed-deadline suppression and exact dispatch indexes.
            deadlines: self.deadlines.clone(),
            scheduled: self.scheduled.clone(),
            active: self.active.clone(),
            active_controls: self.active_controls.clone(),
            held: self.held.clone(),
            held_contacts: self.held_contacts.clone(),
            contact_enabled: self.contact_enabled,
            hazards: self.hazards.clone(),
            effective_time: self.effective_time,
            initial_configuration: self.initial_configuration.clone(),
        };
        clone.canonical_state_bytes()?;
        Ok(clone)
    }
}
