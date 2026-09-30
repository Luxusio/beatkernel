use crate::{
    chart::TimedObject,
    input::{ButtonState, GameControlId, GameInputEvent, PhysicalInputEvent},
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
        ))
    }
}

#[derive(Clone)]
struct ButtonInteraction {
    start: Timestamp,
    end: Option<Timestamp>,
    control: GameControlId,
    state: InteractionState,
    owner: Option<InputOwner>,
}

impl ButtonInteraction {
    fn new(start: Timestamp, end: Option<Timestamp>, control: GameControlId) -> Self {
        Self {
            start,
            end,
            control,
            state: InteractionState::Pending,
            owner: None,
        }
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
        let mut bytes = crate::judge::snapshot::Encoder::new(b"button-interaction/v1");
        bytes.i64(self.start.as_nanos());
        bytes.option(self.end, |bytes, end| bytes.i64(end.as_nanos()));
        bytes.u32(self.control.0);
        bytes.u8(match self.state {
            InteractionState::Pending => 0,
            InteractionState::Active => 1,
            InteractionState::Completed => 2,
        });
        bytes.option(self.owner, |bytes, owner| bytes.owner(owner));
        Some(bytes.finish())
    }

    fn state(&self) -> InteractionState {
        self.state
    }

    fn accepts_input(&self, event: &GameInputEvent, context: &InteractionContext<'_>) -> bool {
        if event.game_control != self.control {
            return false;
        }
        let PhysicalInputEvent::Button(button) = &event.physical else {
            return false;
        };
        match self.state {
            InteractionState::Pending => {
                button.state == ButtonState::Down
                    && Self::within(
                        i128::from(context.song_time.as_nanos())
                            - i128::from(self.start.as_nanos()),
                        context.profile,
                    )
            }
            InteractionState::Active => {
                button.state == ButtonState::Up
                    && self.owner
                        == Some(InputOwner {
                            source: button.meta.source,
                            physical: button.control,
                            game_control: event.game_control,
                        })
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
        let PhysicalInputEvent::Button(button) = &event.physical else {
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
                self.owner = Some(InputOwner {
                    source: button.meta.source,
                    physical: button.control,
                    game_control: event.game_control,
                });
            } else {
                self.finish();
            }
            return Self::output(stage, Self::hit(grade, delta));
        }
        let end = self.end.expect("only held interactions become active");
        let delta = i128::from(context.song_time.as_nanos()) - i128::from(end.as_nanos());
        let outcome = if delta < -i128::from(context.profile.max_early().as_nanos()) {
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
