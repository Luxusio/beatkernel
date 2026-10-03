use crate::{
    chart::TimedObject,
    input::{
        ButtonState, ContactId, GameControlId, GameInputEvent, PhysicalInputEvent, TouchPhase,
    },
    judge::{JudgeError, JudgeOutcome, JudgeProfile, JudgeStage, MissReason},
    time::{Duration, Timestamp},
};

use super::{
    ActiveInteraction, BeginContext, InputOwner, InteractionContext, InteractionEvaluator,
    InteractionOutput, InteractionResult, InteractionState, StartEligibility,
};

/// A point interaction completed by a fresh button press.
#[derive(Clone, Copy, Debug, Default)]
pub struct InstantEvaluator;

/// A button interaction requiring a graded press and an owner release.
#[derive(Clone, Copy, Debug, Default)]
pub struct HoldEvaluator;

/// A point interaction completed by a fresh button or touch-contact press.
#[derive(Clone, Copy, Debug, Default)]
pub struct PressInstantEvaluator;

/// A button/contact press followed by its owning release; cancellation misses.
#[derive(Clone, Copy, Debug, Default)]
pub struct PressHoldEvaluator;

impl InteractionEvaluator for InstantEvaluator {
    fn start_eligibility(&self) -> StartEligibility {
        StartEligibility::ProfileButtonPress
    }

    fn validate(&self, object: &TimedObject, _: &JudgeProfile) -> Result<(), JudgeError> {
        if object.time.end.is_some() {
            return Err(JudgeError::InvalidObjectRange { object: object.id });
        }
        Ok(())
    }

    fn begin(
        &self,
        object: &TimedObject,
        context: &BeginContext<'_>,
    ) -> Box<dyn ActiveInteraction> {
        Box::new(ButtonInteraction::new(
            object.time.start,
            None,
            context.control,
            false,
        ))
    }
}

impl InteractionEvaluator for HoldEvaluator {
    fn start_eligibility(&self) -> StartEligibility {
        StartEligibility::ProfileButtonPress
    }

    fn validate(&self, object: &TimedObject, _: &JudgeProfile) -> Result<(), JudgeError> {
        if object.time.end.is_none_or(|end| end <= object.time.start) {
            return Err(JudgeError::InvalidObjectRange { object: object.id });
        }
        Ok(())
    }

    fn begin(
        &self,
        object: &TimedObject,
        context: &BeginContext<'_>,
    ) -> Box<dyn ActiveInteraction> {
        let end = object
            .time
            .end
            .expect("HoldEvaluator::begin requires validated range");
        Box::new(ButtonInteraction::new(
            object.time.start,
            Some(end),
            context.control,
            false,
        ))
    }
}

impl InteractionEvaluator for PressInstantEvaluator {
    fn start_eligibility(&self) -> StartEligibility {
        StartEligibility::ProfilePress
    }

    fn validate(&self, object: &TimedObject, profile: &JudgeProfile) -> Result<(), JudgeError> {
        InstantEvaluator.validate(object, profile)
    }

    fn begin(
        &self,
        object: &TimedObject,
        context: &BeginContext<'_>,
    ) -> Box<dyn ActiveInteraction> {
        Box::new(ButtonInteraction::new(
            object.time.start,
            None,
            context.control,
            true,
        ))
    }
}

impl InteractionEvaluator for PressHoldEvaluator {
    fn start_eligibility(&self) -> StartEligibility {
        StartEligibility::ProfilePress
    }

    fn validate(&self, object: &TimedObject, profile: &JudgeProfile) -> Result<(), JudgeError> {
        HoldEvaluator.validate(object, profile)
    }

    fn begin(
        &self,
        object: &TimedObject,
        context: &BeginContext<'_>,
    ) -> Box<dyn ActiveInteraction> {
        let end = object
            .time
            .end
            .expect("PressHoldEvaluator::begin requires validated range");
        Box::new(ButtonInteraction::new(
            object.time.start,
            Some(end),
            context.control,
            true,
        ))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PressOwner {
    Button(InputOwner),
    Contact(InputOwner, ContactId),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PressAction {
    Down,
    Up,
    Cancel,
}

#[derive(Clone)]
struct ButtonInteraction {
    start: Timestamp,
    end: Option<Timestamp>,
    control: GameControlId,
    state: InteractionState,
    owner: Option<PressOwner>,
    accepts_contact: bool,
}

impl ButtonInteraction {
    fn new(
        start: Timestamp,
        end: Option<Timestamp>,
        control: GameControlId,
        accepts_contact: bool,
    ) -> Self {
        Self {
            start,
            end,
            control,
            state: InteractionState::Pending,
            owner: None,
            accepts_contact,
        }
    }

    fn press_input(&self, event: &GameInputEvent) -> Option<(PressOwner, PressAction)> {
        let (physical, contact, action) = match &event.physical {
            PhysicalInputEvent::Button(button) => (
                button.control,
                None,
                match button.state {
                    ButtonState::Down => PressAction::Down,
                    ButtonState::Up => PressAction::Up,
                    ButtonState::Repeat => return None,
                },
            ),
            PhysicalInputEvent::Touch(touch) if self.accepts_contact => (
                touch.control,
                Some(touch.contact),
                match touch.phase {
                    TouchPhase::Down => PressAction::Down,
                    TouchPhase::Up => PressAction::Up,
                    TouchPhase::Cancel => PressAction::Cancel,
                    TouchPhase::Move => return None,
                },
            ),
            _ => return None,
        };
        let owner = InputOwner {
            source: event.physical.meta().source,
            physical,
            game_control: event.game_control,
        };
        Some((
            contact.map_or(PressOwner::Button(owner), |contact| {
                PressOwner::Contact(owner, contact)
            }),
            action,
        ))
    }

    fn head_stage(&self) -> JudgeStage {
        if self.end.is_some() {
            JudgeStage::HoldHead
        } else {
            JudgeStage::Instant
        }
    }

    fn within(delta: i128, profile: &JudgeProfile) -> bool {
        delta >= -i128::from(profile.max_early().as_nanos())
            && delta <= i128::from(profile.max_late().as_nanos())
    }

    fn output(stage: JudgeStage, outcome: JudgeOutcome) -> InteractionOutput {
        InteractionOutput {
            results: vec![InteractionResult { stage, outcome }],
        }
    }

    fn finish(&mut self) {
        self.state = InteractionState::Completed;
        self.owner = None;
    }

    fn hit(grade: crate::judge::JudgeGrade, delta: i128) -> JudgeOutcome {
        // Only called within validated nonnegative i64 window bounds.
        JudgeOutcome::Hit {
            grade,
            delta: Duration::from_nanos(
                i64::try_from(delta).expect("eligible timing error fits validated window bounds"),
            ),
        }
    }
}

impl ActiveInteraction for ButtonInteraction {
    fn snapshot_clone(&self) -> Option<Box<dyn ActiveInteraction>> {
        Some(Box::new(self.clone()))
    }

    fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        let schema: &[u8] = if self.accepts_contact {
            b"press-interaction/v1"
        } else {
            b"button-interaction/v1"
        };
        let mut bytes = crate::judge::snapshot::Encoder::new(schema);
        bytes.i64(self.start.as_nanos());
        bytes.option(self.end, |bytes, end| bytes.i64(end.as_nanos()));
        bytes.u32(self.control.0);
        bytes.u8(match self.state {
            InteractionState::Pending => 0,
            InteractionState::Active => 1,
            InteractionState::Completed => 2,
        });
        if self.accepts_contact {
            bytes.option(self.owner, |bytes, owner| match owner {
                PressOwner::Button(owner) => {
                    bytes.u8(0);
                    bytes.owner(owner);
                }
                PressOwner::Contact(owner, contact) => {
                    bytes.u8(1);
                    bytes.owner(owner);
                    bytes.u64(contact.0);
                }
            });
        } else {
            // Keep the original schema and owner bytes for button-only rules.
            let owner = self.owner.map(|owner| match owner {
                PressOwner::Button(owner) => owner,
                PressOwner::Contact(..) => {
                    unreachable!("button-only interactions cannot own contacts")
                }
            });
            bytes.option(owner, |bytes, owner| bytes.owner(owner));
        }
        Some(bytes.finish())
    }

    fn state(&self) -> InteractionState {
        self.state
    }

    fn accepts_input(&self, event: &GameInputEvent, context: &InteractionContext<'_>) -> bool {
        if event.game_control != self.control {
            return false;
        }
        let Some((owner, action)) = self.press_input(event) else {
            return false;
        };
        match self.state {
            InteractionState::Pending => {
                action == PressAction::Down
                    && Self::within(
                        i128::from(context.song_time.as_nanos())
                            - i128::from(self.start.as_nanos()),
                        context.profile,
                    )
            }
            InteractionState::Active => {
                matches!(action, PressAction::Up | PressAction::Cancel) && self.owner == Some(owner)
            }
            InteractionState::Completed => false,
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
        let Some((owner, action)) = self.press_input(event) else {
            unreachable!()
        };
        if self.state == InteractionState::Pending {
            let delta =
                i128::from(context.song_time.as_nanos()) - i128::from(self.start.as_nanos());
            let Some(grade) = context.policy.grade(delta, context.profile) else {
                return InteractionOutput::default();
            };
            let stage = self.head_stage();
            if self.end.is_some() {
                self.state = InteractionState::Active;
                self.owner = Some(owner);
            } else {
                self.finish();
            }
            return Self::output(stage, Self::hit(grade, delta));
        }
        let end = self.end.expect("only held interactions become active");
        let delta = i128::from(context.song_time.as_nanos()) - i128::from(end.as_nanos());
        let outcome = if action == PressAction::Cancel {
            JudgeOutcome::Miss {
                reason: MissReason::RejectedInput,
            }
        } else if delta < -i128::from(context.profile.max_early().as_nanos()) {
            JudgeOutcome::Miss {
                reason: MissReason::EarlyRelease,
            }
        } else if delta > i128::from(context.profile.max_late().as_nanos()) {
            JudgeOutcome::Miss {
                reason: MissReason::TailTimeout,
            }
        } else if let Some(grade) = context.policy.grade(delta, context.profile) {
            Self::hit(grade, delta)
        } else {
            JudgeOutcome::Miss {
                reason: MissReason::RejectedInput,
            }
        };
        self.finish();
        Self::output(JudgeStage::HoldTail, outcome)
    }

    fn advance_to(
        &mut self,
        song_time: Timestamp,
        context: &InteractionContext<'_>,
    ) -> InteractionOutput {
        if self
            .deadline(context.profile)
            .is_none_or(|deadline| deadline >= i128::from(song_time.as_nanos()))
        {
            return InteractionOutput::default();
        }
        let (stage, reason) = if self.state == InteractionState::Active {
            (JudgeStage::HoldTail, MissReason::TailTimeout)
        } else {
            (self.head_stage(), MissReason::HeadTimeout)
        };
        self.finish();
        Self::output(stage, JudgeOutcome::Miss { reason })
    }

    fn deadline(&self, profile: &JudgeProfile) -> Option<i128> {
        let target = match self.state {
            InteractionState::Pending => self.start,
            InteractionState::Active => self.end.expect("only held interactions become active"),
            InteractionState::Completed => return None,
        };
        Some(i128::from(target.as_nanos()) + i128::from(profile.max_late().as_nanos()))
    }
}
