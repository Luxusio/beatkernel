use super::{
    ActiveInteraction, BeginContext, InputOwner, InteractionContext, InteractionEvaluator,
    InteractionOutput, InteractionResult, InteractionState,
};
use crate::{
    chart::TimedObject,
    input::{
        AxisMode, ButtonState, ContactId, GameControlId, GameInputEvent, PhysicalInputEvent,
        PointerMode, TouchPhase,
    },
    judge::{snapshot::Encoder, JudgeError, JudgeOutcome, JudgeProfile, JudgeStage, MissReason},
    time::{Duration, Timestamp},
};
use std::collections::BTreeMap;

/// A ranged interaction completed by a minimum number of fresh button presses.
#[derive(Clone, Debug)]
pub struct RepeatedEvaluator {
    /// Positive required press count.
    pub minimum_hits: u32,
}

/// Typed input accepted by a configurable tracking path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackingInput {
    /// Absolute position or accumulated relative displacement in x.
    Axis,
    /// One acquired touch contact with device/control/contact ownership.
    Contact,
    /// Absolute or accumulated relative two-dimensional pointer.
    Pointer,
    /// Three-dimensional pose position; orientation remains in provenance.
    Pose,
}

/// Ranged tracking against a uniformly parameterized caller path.
#[derive(Clone, Debug)]
pub struct TrackingEvaluator {
    /// Typed sample model.
    pub input: TrackingInput,
    /// At least one finite three-dimensional target point.
    pub points: Vec<[f32; 3]>,
    /// Nonnegative Euclidean tolerance in caller coordinates.
    pub tolerance: f32,
    /// Positive maximum time between accepted observations.
    pub max_gap: Duration,
}

/// A fresh primary trigger gated by held logical prerequisite controls.
#[derive(Clone, Debug)]
pub struct CompositeEvaluator {
    /// Distinct prerequisite controls; empty sets are rejected.
    pub required_controls: Vec<GameControlId>,
    /// Require each prerequisite to be held by the trigger's source device.
    pub same_device: bool,
}

fn ranged(object: &TimedObject) -> Result<Timestamp, JudgeError> {
    object
        .time
        .end
        .filter(|end| *end > object.time.start)
        .ok_or(JudgeError::InvalidObjectRange { object: object.id })
}
fn invalid(object: &TimedObject) -> JudgeError {
    JudgeError::InvalidObjectRange { object: object.id }
}
fn output(outcome: JudgeOutcome) -> InteractionOutput {
    InteractionOutput {
        results: vec![InteractionResult {
            stage: JudgeStage::Custom(0),
            outcome,
        }],
    }
}
fn miss(reason: MissReason) -> InteractionOutput {
    output(JudgeOutcome::Miss { reason })
}
fn success(context: &InteractionContext<'_>, delta: i128) -> InteractionOutput {
    match (
        context.policy.grade(delta, context.profile),
        i64::try_from(delta),
    ) {
        (Some(grade), Ok(delta)) => output(JudgeOutcome::Hit {
            grade,
            delta: Duration::from_nanos(delta),
        }),
        _ => miss(MissReason::RejectedInput),
    }
}
fn owner(event: &GameInputEvent) -> Option<InputOwner> {
    let physical = match &event.physical {
        PhysicalInputEvent::Button(v) => v.control,
        PhysicalInputEvent::Axis(v) => v.control,
        PhysicalInputEvent::Touch(v) => v.control,
        PhysicalInputEvent::Pointer(v) => v.control,
        PhysicalInputEvent::Pose(v) => v.control,
        _ => return None,
    };
    Some(InputOwner {
        source: event.physical.meta().source,
        physical,
        game_control: event.game_control,
    })
}
fn state_bytes(out: &mut Encoder, state: InteractionState) {
    out.u8(match state {
        InteractionState::Pending => 0,
        InteractionState::Active => 1,
        InteractionState::Completed => 2,
    });
}
fn owner_key(owner: InputOwner) -> Vec<u8> {
    let mut out = Encoder::new(b"advanced-owner/v1");
    out.owner(owner);
    out.finish()
}

impl InteractionEvaluator for RepeatedEvaluator {
    fn validate(&self, object: &TimedObject, _: &JudgeProfile) -> Result<(), JudgeError> {
        ranged(object)?;
        if self.minimum_hits == 0 {
            return Err(invalid(object));
        }
        Ok(())
    }
    fn begin(
        &self,
        object: &TimedObject,
        context: &BeginContext<'_>,
    ) -> Box<dyn ActiveInteraction> {
        Box::new(Repeated {
            start: object.time.start,
            end: object.time.end.expect("validated range"),
            control: context.control,
            required: self.minimum_hits,
            hits: 0,
            held: BTreeMap::new(),
            state: InteractionState::Pending,
        })
    }
}

#[derive(Clone)]
struct Repeated {
    start: Timestamp,
    end: Timestamp,
    control: GameControlId,
    required: u32,
    hits: u32,
    held: BTreeMap<Vec<u8>, InputOwner>,
    state: InteractionState,
}
impl ActiveInteraction for Repeated {
    fn snapshot_clone(&self) -> Option<Box<dyn ActiveInteraction>> {
        Some(Box::new(self.clone()))
    }
    fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        let mut out = Encoder::new(b"repeated/v1");
        out.i64(self.start.as_nanos());
        out.i64(self.end.as_nanos());
        out.u32(self.control.0);
        out.u32(self.required);
        out.u32(self.hits);
        state_bytes(&mut out, self.state);
        out.u64(self.held.len() as u64);
        for value in self.held.values() {
            out.owner(*value);
        }
        Some(out.finish())
    }
    fn state(&self) -> InteractionState {
        self.state
    }
    fn accepts_input(&self, event: &GameInputEvent, context: &InteractionContext<'_>) -> bool {
        self.state != InteractionState::Completed
            && event.game_control == self.control
            && matches!(event.physical, PhysicalInputEvent::Button(_))
            && context.song_time >= self.start
            && context.song_time <= self.end
    }
    fn on_input(
        &mut self,
        event: &GameInputEvent,
        context: &InteractionContext<'_>,
    ) -> InteractionOutput {
        if !self.accepts_input(event, context) {
            return InteractionOutput::default();
        }
        let PhysicalInputEvent::Button(button) = &event.physical else {
            return InteractionOutput::default();
        };
        let source = owner(event).expect("button owner");
        let key = owner_key(source);
        match button.state {
            ButtonState::Down => {
                if self.held.insert(key, source).is_none() {
                    self.hits = self.hits.saturating_add(1);
                    self.state = InteractionState::Active;
                }
            }
            ButtonState::Up => {
                self.held.remove(&key);
            }
            ButtonState::Repeat => {}
        }
        if self.hits >= self.required {
            self.state = InteractionState::Completed;
            success(context, 0)
        } else {
            InteractionOutput::default()
        }
    }
    fn advance_to(
        &mut self,
        song_time: Timestamp,
        _: &InteractionContext<'_>,
    ) -> InteractionOutput {
        if self.state != InteractionState::Completed && song_time > self.end {
            self.state = InteractionState::Completed;
            miss(MissReason::TailTimeout)
        } else {
            InteractionOutput::default()
        }
    }
    fn deadline(&self, _: &JudgeProfile) -> Option<i128> {
        (self.state != InteractionState::Completed).then_some(i128::from(self.end.as_nanos()))
    }
}

impl InteractionEvaluator for CompositeEvaluator {
    fn validate(&self, object: &TimedObject, _: &JudgeProfile) -> Result<(), JudgeError> {
        if object.time.end.is_some()
            || self.required_controls.is_empty()
            || self
                .required_controls
                .iter()
                .enumerate()
                .any(|(index, control)| self.required_controls[..index].contains(control))
        {
            return Err(invalid(object));
        }
        Ok(())
    }
    fn begin(
        &self,
        object: &TimedObject,
        context: &BeginContext<'_>,
    ) -> Box<dyn ActiveInteraction> {
        Box::new(Composite {
            target: object.time.start,
            trigger: context.control,
            required: self.required_controls.clone(),
            same_device: self.same_device,
            held: BTreeMap::new(),
            state: InteractionState::Active,
        })
    }
}

#[derive(Clone)]
struct Composite {
    target: Timestamp,
    trigger: GameControlId,
    required: Vec<GameControlId>,
    same_device: bool,
    held: BTreeMap<Vec<u8>, InputOwner>,
    state: InteractionState,
}
impl ActiveInteraction for Composite {
    fn additional_controls(&self) -> &[GameControlId] {
        &self.required
    }
    fn snapshot_clone(&self) -> Option<Box<dyn ActiveInteraction>> {
        Some(Box::new(self.clone()))
    }
    fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        let mut out = Encoder::new(b"composite/v1");
        out.i64(self.target.as_nanos());
        out.u32(self.trigger.0);
        out.u8(u8::from(self.same_device));
        out.u64(self.required.len() as u64);
        for control in &self.required {
            out.u32(control.0);
        }
        state_bytes(&mut out, self.state);
        out.u64(self.held.len() as u64);
        for source in self.held.values() {
            out.owner(*source);
        }
        Some(out.finish())
    }
    fn state(&self) -> InteractionState {
        self.state
    }
    fn accepts_input(&self, event: &GameInputEvent, _: &InteractionContext<'_>) -> bool {
        self.state != InteractionState::Completed
            && (event.game_control == self.trigger || self.required.contains(&event.game_control))
            && matches!(event.physical, PhysicalInputEvent::Button(_))
    }
    fn on_input(
        &mut self,
        event: &GameInputEvent,
        context: &InteractionContext<'_>,
    ) -> InteractionOutput {
        if !self.accepts_input(event, context) {
            return InteractionOutput::default();
        }
        let PhysicalInputEvent::Button(button) = &event.physical else {
            return InteractionOutput::default();
        };
        let source = owner(event).expect("button owner");
        let key = owner_key(source);
        let fresh = match button.state {
            ButtonState::Down => self.held.insert(key, source).is_none(),
            ButtonState::Up => {
                self.held.remove(&key);
                false
            }
            ButtonState::Repeat => false,
        };
        let delta = i128::from(context.song_time.as_nanos()) - i128::from(self.target.as_nanos());
        if fresh
            && event.game_control == self.trigger
            && delta >= -i128::from(context.profile.max_early().as_nanos())
            && delta <= i128::from(context.profile.max_late().as_nanos())
            && self.required.iter().all(|required| {
                self.held.values().any(|held| {
                    held.game_control == *required
                        && (!self.same_device || held.source == source.source)
                })
            })
        {
            if context.policy.grade(delta, context.profile).is_some() {
                self.state = InteractionState::Completed;
                return success(context, delta);
            }
        }
        InteractionOutput::default()
    }
    fn advance_to(
        &mut self,
        song_time: Timestamp,
        context: &InteractionContext<'_>,
    ) -> InteractionOutput {
        if self.state != InteractionState::Completed
            && i128::from(song_time.as_nanos())
                > self.deadline(context.profile).expect("armed composite")
        {
            self.state = InteractionState::Completed;
            miss(MissReason::HeadTimeout)
        } else {
            InteractionOutput::default()
        }
    }
    fn deadline(&self, profile: &JudgeProfile) -> Option<i128> {
        (self.state != InteractionState::Completed).then_some(
            i128::from(self.target.as_nanos()) + i128::from(profile.max_late().as_nanos()),
        )
    }
}

impl InteractionEvaluator for TrackingEvaluator {
    fn validate(&self, object: &TimedObject, _: &JudgeProfile) -> Result<(), JudgeError> {
        ranged(object)?;
        if self.points.is_empty()
            || self.points.iter().flatten().any(|v| !v.is_finite())
            || !self.tolerance.is_finite()
            || self.tolerance < 0.0
            || self.max_gap.as_nanos() <= 0
        {
            return Err(invalid(object));
        }
        Ok(())
    }
    fn begin(
        &self,
        object: &TimedObject,
        context: &BeginContext<'_>,
    ) -> Box<dyn ActiveInteraction> {
        Box::new(Tracking {
            settings: self.clone(),
            start: object.time.start,
            end: object.time.end.expect("validated range"),
            control: context.control,
            state: InteractionState::Pending,
            owner: None,
            contact: None,
            last_time: None,
            position: [0.0; 3],
        })
    }
}

#[derive(Clone)]
struct Tracking {
    settings: TrackingEvaluator,
    start: Timestamp,
    end: Timestamp,
    control: GameControlId,
    state: InteractionState,
    owner: Option<InputOwner>,
    contact: Option<ContactId>,
    last_time: Option<Timestamp>,
    position: [f32; 3],
}
impl Tracking {
    fn target(&self, time: Timestamp) -> [f64; 3] {
        let fraction = ((i128::from(time.as_nanos()) - i128::from(self.start.as_nanos())) as f64
            / (i128::from(self.end.as_nanos()) - i128::from(self.start.as_nanos())) as f64)
            .clamp(0.0, 1.0);
        let coordinate = fraction * (self.settings.points.len() - 1) as f64;
        let index = coordinate.floor() as usize;
        let next = (index + 1).min(self.settings.points.len() - 1);
        let weight = coordinate - index as f64;
        std::array::from_fn(|axis| {
            f64::from(self.settings.points[index][axis]) * (1.0 - weight)
                + f64::from(self.settings.points[next][axis]) * weight
        })
    }
    fn finish(&mut self, result: InteractionOutput) -> InteractionOutput {
        self.state = InteractionState::Completed;
        result
    }
    fn gap_exceeded(&self, time: Timestamp) -> bool {
        self.last_time.is_some_and(|last| {
            i128::from(time.as_nanos()) - i128::from(last.as_nanos())
                > i128::from(self.settings.max_gap.as_nanos())
        })
    }
}
impl ActiveInteraction for Tracking {
    fn snapshot_clone(&self) -> Option<Box<dyn ActiveInteraction>> {
        Some(Box::new(self.clone()))
    }
    fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        let mut out = Encoder::new(b"tracking/v1");
        out.u8(match self.settings.input {
            TrackingInput::Axis => 0,
            TrackingInput::Contact => 1,
            TrackingInput::Pointer => 2,
            TrackingInput::Pose => 3,
        });
        out.u32(self.settings.tolerance.to_bits());
        out.i64(self.settings.max_gap.as_nanos());
        out.u64(self.settings.points.len() as u64);
        for point in &self.settings.points {
            for value in point {
                out.u32(value.to_bits());
            }
        }
        out.i64(self.start.as_nanos());
        out.i64(self.end.as_nanos());
        out.u32(self.control.0);
        state_bytes(&mut out, self.state);
        out.option(self.owner, Encoder::owner);
        out.option(self.contact, |out, contact| out.u64(contact.0));
        out.option(self.last_time, |out, time| out.i64(time.as_nanos()));
        for value in self.position {
            out.u32(value.to_bits());
        }
        Some(out.finish())
    }
    fn state(&self) -> InteractionState {
        self.state
    }
    fn accepts_input(&self, event: &GameInputEvent, context: &InteractionContext<'_>) -> bool {
        if self.state == InteractionState::Completed
            || event.game_control != self.control
            || self
                .owner
                .is_some_and(|acquired| Some(acquired) != owner(event))
        {
            return false;
        }
        if self.state == InteractionState::Pending
            && (context.song_time < self.start
                || i128::from(context.song_time.as_nanos())
                    > i128::from(self.start.as_nanos())
                        + i128::from(context.profile.max_late().as_nanos()))
        {
            return false;
        }
        match (&self.settings.input, &event.physical) {
            (TrackingInput::Axis, PhysicalInputEvent::Axis(v)) => v.value.is_finite(),
            (TrackingInput::Contact, PhysicalInputEvent::Touch(v)) => {
                v.position.x.is_finite()
                    && v.position.y.is_finite()
                    && self.contact.is_none_or(|contact| contact == v.contact)
                    && (self.state != InteractionState::Pending || v.phase == TouchPhase::Down)
            }
            (TrackingInput::Pointer, PhysicalInputEvent::Pointer(v)) => {
                v.position.x.is_finite() && v.position.y.is_finite()
            }
            (TrackingInput::Pose, PhysicalInputEvent::Pose(v)) => {
                [v.position.x, v.position.y, v.position.z]
                    .iter()
                    .all(|value| value.is_finite())
            }
            _ => false,
        }
    }
    fn on_input(
        &mut self,
        event: &GameInputEvent,
        context: &InteractionContext<'_>,
    ) -> InteractionOutput {
        if !self.accepts_input(event, context) {
            return InteractionOutput::default();
        }
        if self.gap_exceeded(context.song_time) {
            return self.finish(miss(MissReason::TailTimeout));
        }
        match &event.physical {
            PhysicalInputEvent::Axis(v) => {
                self.position[0] = match v.mode {
                    AxisMode::Absolute => v.value,
                    AxisMode::Relative => self.position[0] + v.value,
                };
            }
            PhysicalInputEvent::Touch(v) => {
                if v.phase == TouchPhase::Cancel
                    || (v.phase == TouchPhase::Up && context.song_time < self.end)
                {
                    return self.finish(miss(MissReason::EarlyRelease));
                }
                self.position = [v.position.x, v.position.y, 0.0];
                self.contact = Some(v.contact);
            }
            PhysicalInputEvent::Pointer(v) => match v.mode {
                PointerMode::Absolute => self.position = [v.position.x, v.position.y, 0.0],
                PointerMode::Relative => {
                    self.position[0] += v.position.x;
                    self.position[1] += v.position.y;
                }
            },
            PhysicalInputEvent::Pose(v) => {
                self.position = [v.position.x, v.position.y, v.position.z]
            }
            _ => return InteractionOutput::default(),
        }
        let target = self.target(context.song_time);
        let error: f64 = self
            .position
            .iter()
            .zip(target)
            .map(|(value, target)| (f64::from(*value) - target).powi(2))
            .sum();
        if !error.is_finite() || error > f64::from(self.settings.tolerance).powi(2) {
            return self.finish(miss(MissReason::RejectedInput));
        }
        self.state = InteractionState::Active;
        self.owner = owner(event);
        self.last_time = Some(context.song_time);
        if context.song_time >= self.end {
            self.finish(success(context, 0))
        } else {
            InteractionOutput::default()
        }
    }
    fn advance_to(
        &mut self,
        song_time: Timestamp,
        context: &InteractionContext<'_>,
    ) -> InteractionOutput {
        match self.state {
            InteractionState::Completed => InteractionOutput::default(),
            InteractionState::Pending => {
                if i128::from(song_time.as_nanos())
                    > i128::from(self.start.as_nanos())
                        + i128::from(context.profile.max_late().as_nanos())
                {
                    self.finish(miss(MissReason::HeadTimeout))
                } else {
                    InteractionOutput::default()
                }
            }
            InteractionState::Active => {
                if self.gap_exceeded(song_time) {
                    self.finish(miss(MissReason::TailTimeout))
                } else if song_time >= self.end {
                    self.finish(success(context, 0))
                } else {
                    InteractionOutput::default()
                }
            }
        }
    }
    fn deadline(&self, profile: &JudgeProfile) -> Option<i128> {
        match self.state {
            InteractionState::Completed => None,
            InteractionState::Pending => {
                Some(i128::from(self.start.as_nanos()) + i128::from(profile.max_late().as_nanos()))
            }
            InteractionState::Active => Some(
                (i128::from(self.last_time.expect("active sample").as_nanos())
                    + i128::from(self.settings.max_gap.as_nanos()))
                .min(i128::from(self.end.as_nanos())),
            ),
        }
    }
}
