use std::{
    cmp::Reverse,
    collections::{BTreeSet, BinaryHeap, HashMap, HashSet},
};

use crate::{
    chart::{CompiledChart, ObjectId},
    input::{ButtonState, EventMeta, GameControlId, GameInputEvent, PhysicalInputEvent},
    interaction::{
        ActiveInteraction, BeginContext, InputOwner, InteractionContext, InteractionOutput,
        InteractionState,
    },
    time::Timestamp,
};

use super::{
    Candidate, CandidateResolver, ClosestCandidate, JudgeError, JudgeEvent, JudgePolicy,
    JudgeProfile, Rule, WindowJudgePolicy,
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
    starts: HashMap<GameControlId, Vec<usize>>,
    identities: HashMap<ObjectId, usize>,
    deadlines: BinaryHeap<Reverse<(i128, ObjectId, usize)>>,
    scheduled: Vec<Option<i128>>,
    active: BTreeSet<(ObjectId, usize)>,
    active_controls: HashMap<GameControlId, BTreeSet<(ObjectId, usize)>>,
    held: HashSet<InputOwner>,
    effective_time: Option<Timestamp>,
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

    /// Constructs a judge with caller-selected candidate and grading policies.
    pub fn with_policies(
        chart: CompiledChart,
        rules: Vec<Rule>,
        profile: JudgeProfile,
        resolver: Box<dyn CandidateResolver>,
        policy: Box<dyn JudgePolicy>,
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
            starts.entry(rule.control).or_default().push(index);
            identities.insert(object.id, index);
        }
        let count = interactions.len();
        let mut engine = Self {
            chart,
            profile,
            resolver,
            policy,
            interactions,
            controls,
            starts,
            identities,
            deadlines: BinaryHeap::new(),
            scheduled: vec![None; count],
            active: BTreeSet::new(),
            active_controls: HashMap::new(),
            held: HashSet::new(),
            effective_time: None,
        };
        for index in 0..count {
            engine.refresh(index, None);
        }
        Ok(engine)
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
        let fresh_start = button
            .is_none_or(|(owner, state)| state == ButtonState::Down && !self.held.contains(&owner));
        let candidates = if fresh_start {
            self.candidates(event, time)
        } else {
            Vec::new()
        };
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
        let mut output = Vec::new();
        self.expire(time, &mut output);
        if let Some((owner, state)) = button {
            match state {
                ButtonState::Down => {
                    self.held.insert(owner);
                }
                ButtonState::Up => {
                    self.held.remove(&owner);
                }
                ButtonState::Repeat => {}
            }
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

    /// Reports physical ownership scoped to one bound logical destination.
    pub fn is_held(&self, owner: InputOwner) -> bool {
        self.held.contains(&owner)
    }

    /// Borrows the immutable compiled chart owned by this judge.
    pub const fn chart(&self) -> &CompiledChart {
        &self.chart
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

    fn candidates(&self, event: &GameInputEvent, time: Timestamp) -> Vec<Candidate> {
        let Some(starts) = self.starts.get(&event.game_control) else {
            return Vec::new();
        };
        let now = i128::from(time.as_nanos());
        let first_time = now - i128::from(self.profile.max_late().as_nanos());
        let last_time = now + i128::from(self.profile.max_early().as_nanos());
        let first = starts.partition_point(|&index| {
            i128::from(self.chart.objects()[index].time.start.as_nanos()) < first_time
        });
        let last = starts.partition_point(|&index| {
            i128::from(self.chart.objects()[index].time.start.as_nanos()) <= last_time
        });
        let context = self.context(time);
        starts[first..last]
            .iter()
            .filter_map(|&index| {
                let interaction = &self.interactions[index];
                let object = &self.chart.objects()[index];
                (interaction.state() == InteractionState::Pending
                    && interaction.accepts_input(event, &context))
                .then_some(Candidate {
                    object: object.id,
                    target: object.time.start,
                    delta: now - i128::from(object.time.start.as_nanos()),
                })
            })
            .collect()
    }

    fn refresh(&mut self, index: usize, consumed_deadline: Option<i128>) {
        let key = (self.chart.objects()[index].id, index);
        let control = self.controls[index];
        if self.interactions[index].state() == InteractionState::Active {
            self.active.insert(key);
            self.active_controls.entry(control).or_default().insert(key);
        } else {
            self.active.remove(&key);
            if let Some(indices) = self.active_controls.get_mut(&control) {
                indices.remove(&key);
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
