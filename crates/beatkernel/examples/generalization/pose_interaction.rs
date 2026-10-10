//! Caller-owned pose tracking: a linear unit-diagonal path and identity cone.
//! Samples have Euclidean tolerance 0.001, gap at most 150ns, and finite
//! quaternion squared norm within 0.001 of one with abs(w) >= 0.9.
//! Acquisition requires the origin in the profile head window; completion
//! requires an original qualifying sample at or after the ranged object's end.

use beatkernel::{
    chart::TimedObject,
    input::{GameControlId, GameInputEvent, PhysicalControlId, PhysicalInputEvent, PoseEvent},
    interaction::{
        ActiveInteraction, BeginContext, InputOwner, InteractionContext, InteractionEvaluator,
        InteractionOutput, InteractionResult, InteractionState,
    },
    judge::{JudgeError, JudgeOutcome, JudgeProfile, JudgeStage, MissReason},
    time::{Duration, Timestamp},
};

const TOLERANCE: f32 = 0.001;
const MIN_ABS_W: f32 = 0.9;
const MAX_GAP_NS: i64 = 150;

/// Fixed example policy; orientation remains game-owned rather than kernel policy.
pub struct DerivedPoseEvaluator;

impl InteractionEvaluator for DerivedPoseEvaluator {
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
        Box::new(DerivedPose {
            start: object.time.start,
            end: object.time.end.expect("validated range"),
            control: context.control,
            state: InteractionState::Pending,
            owner: None,
            last_time: None,
            last_position: [0.0; 3],
            last_orientation: [0.0; 4],
        })
    }
}

#[derive(Clone)]
struct DerivedPose {
    start: Timestamp,
    end: Timestamp,
    control: GameControlId,
    state: InteractionState,
    owner: Option<InputOwner>,
    last_time: Option<Timestamp>,
    last_position: [f32; 3],
    last_orientation: [f32; 4],
}

impl DerivedPose {
    fn owner_of(event: &GameInputEvent, pose: &PoseEvent) -> InputOwner {
        InputOwner {
            source: pose.meta.source,
            physical: pose.control,
            game_control: event.game_control,
        }
    }

    fn qualifies(&self, pose: &PoseEvent, time: Timestamp) -> bool {
        let position = [pose.position.x, pose.position.y, pose.position.z];
        let orientation = [
            pose.orientation.x,
            pose.orientation.y,
            pose.orientation.z,
            pose.orientation.w,
        ];
        if position.iter().chain(&orientation).any(|v| !v.is_finite()) {
            return false;
        }
        let norm: f64 = orientation.iter().map(|v| f64::from(*v).powi(2)).sum();
        if (norm - 1.0).abs() > f64::from(TOLERANCE) || pose.orientation.w.abs() < MIN_ABS_W {
            return false;
        }
        let target = if self.state == InteractionState::Pending {
            0.0
        } else {
            ((i128::from(time.as_nanos()) - i128::from(self.start.as_nanos())) as f64
                / (i128::from(self.end.as_nanos()) - i128::from(self.start.as_nanos())) as f64)
                .clamp(0.0, 1.0)
        };
        let error: f64 = position
            .iter()
            .map(|v| (f64::from(*v) - target).powi(2))
            .sum();
        error <= f64::from(TOLERANCE).powi(2)
    }

    fn finish(&mut self, outcome: JudgeOutcome) -> InteractionOutput {
        self.state = InteractionState::Completed;
        InteractionOutput {
            results: vec![InteractionResult {
                stage: JudgeStage::Custom(0),
                outcome,
            }],
        }
    }

    fn miss(&mut self, reason: MissReason) -> InteractionOutput {
        self.finish(JudgeOutcome::Miss { reason })
    }
}

impl ActiveInteraction for DerivedPose {
    fn snapshot_clone(&self) -> Option<Box<dyn ActiveInteraction>> {
        Some(Box::new(self.clone()))
    }

    fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        let mut bytes = b"example-derived-pose/v1:".to_vec();
        // Include the complete fixed criterion as well as its schema identity.
        for value in [0.0_f32, 0.0, 0.0, 1.0, 1.0, 1.0, TOLERANCE, MIN_ABS_W] {
            bytes.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        bytes.extend_from_slice(&MAX_GAP_NS.to_le_bytes());
        bytes.extend_from_slice(&self.start.as_nanos().to_le_bytes());
        bytes.extend_from_slice(&self.end.as_nanos().to_le_bytes());
        bytes.extend_from_slice(&self.control.0.to_le_bytes());
        bytes.push(match self.state {
            InteractionState::Pending => 0,
            InteractionState::Active => 1,
            InteractionState::Completed => 2,
        });
        match self.owner {
            None => bytes.push(0),
            Some(owner) => {
                bytes.push(1);
                bytes.extend_from_slice(&owner.source.0.to_le_bytes());
                bytes.extend_from_slice(&owner.game_control.0.to_le_bytes());
                match owner.physical {
                    PhysicalControlId::HidUsage { usage_page, usage } => {
                        bytes.push(0);
                        bytes.extend_from_slice(&usage_page.to_le_bytes());
                        bytes.extend_from_slice(&usage.to_le_bytes());
                    }
                    PhysicalControlId::Native { backend, code } => {
                        bytes.push(1);
                        bytes.extend_from_slice(&backend.0.to_le_bytes());
                        bytes.extend_from_slice(&code.to_le_bytes());
                    }
                    PhysicalControlId::Vendor { namespace, code } => {
                        bytes.push(2);
                        bytes.extend_from_slice(&namespace.0.to_le_bytes());
                        bytes.extend_from_slice(&code.to_le_bytes());
                    }
                }
            }
        }
        match self.last_time {
            None => bytes.push(0),
            Some(time) => {
                bytes.push(1);
                bytes.extend_from_slice(&time.as_nanos().to_le_bytes());
            }
        }
        for value in self.last_position.iter().chain(&self.last_orientation) {
            bytes.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        Some(bytes)
    }

    fn state(&self) -> InteractionState {
        self.state
    }

    fn accepts_input(&self, event: &GameInputEvent, context: &InteractionContext<'_>) -> bool {
        if event.game_control != self.control || self.state == InteractionState::Completed {
            return false;
        }
        let PhysicalInputEvent::Pose(pose) = &event.physical else {
            return false;
        };
        match self.state {
            InteractionState::Pending => {
                context
                    .profile
                    .grade(
                        i128::from(context.song_time.as_nanos())
                            - i128::from(self.start.as_nanos()),
                    )
                    .is_some()
                    && self.qualifies(pose, context.song_time)
            }
            // Invalid samples of the owner must reach on_input to fail explicitly.
            InteractionState::Active => self.owner == Some(Self::owner_of(event, pose)),
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
        let PhysicalInputEvent::Pose(pose) = &event.physical else {
            return InteractionOutput::default();
        };
        if self
            .deadline(context.profile)
            .is_some_and(|deadline| i128::from(context.song_time.as_nanos()) > deadline)
        {
            return self.miss(MissReason::TailTimeout);
        }
        if !self.qualifies(pose, context.song_time) {
            return self.miss(MissReason::RejectedInput);
        }
        let acquiring = self.state == InteractionState::Pending;
        self.state = InteractionState::Active;
        self.owner = Some(Self::owner_of(event, pose));
        self.last_time = Some(context.song_time);
        self.last_position = [pose.position.x, pose.position.y, pose.position.z];
        self.last_orientation = [
            pose.orientation.x,
            pose.orientation.y,
            pose.orientation.z,
            pose.orientation.w,
        ];
        if !acquiring && context.song_time >= self.end {
            let delta = i128::from(context.song_time.as_nanos()) - i128::from(self.end.as_nanos());
            return match (
                context.policy.grade(delta, context.profile),
                i64::try_from(delta),
            ) {
                (Some(grade), Ok(delta)) => self.finish(JudgeOutcome::Hit {
                    grade,
                    delta: Duration::from_nanos(delta),
                }),
                _ => self.miss(MissReason::RejectedInput),
            };
        }
        InteractionOutput::default()
    }

    fn advance_to(
        &mut self,
        song_time: Timestamp,
        context: &InteractionContext<'_>,
    ) -> InteractionOutput {
        if self
            .deadline(context.profile)
            .is_some_and(|deadline| i128::from(song_time.as_nanos()) > deadline)
        {
            self.miss(if self.state == InteractionState::Pending {
                MissReason::HeadTimeout
            } else {
                MissReason::TailTimeout
            })
        } else {
            InteractionOutput::default()
        }
    }

    fn deadline(&self, profile: &JudgeProfile) -> Option<i128> {
        match self.state {
            InteractionState::Pending => {
                Some(i128::from(self.start.as_nanos()) + i128::from(profile.max_late().as_nanos()))
            }
            InteractionState::Active => Some(
                (i128::from(self.last_time.expect("active sample").as_nanos())
                    + i128::from(MAX_GAP_NS))
                .min(i128::from(self.end.as_nanos()) + i128::from(profile.max_late().as_nanos())),
            ),
            InteractionState::Completed => None,
        }
    }
}
