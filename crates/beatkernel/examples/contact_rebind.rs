//! Game-owned contact lifecycle policy using the existing custom interaction API.
//!
//! This sustain fixture uses a unit-square region, not the built-in tracking path.
//! A normal early release can either fail or permit same-surface reacquisition.
use beatkernel::{
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, TimedObject,
        VisualId,
    },
    input::{
        BackendId, ContactId, DeviceId, EventMeta, GameControlId, GameInputEvent,
        PhysicalControlId, PhysicalInputEvent, Position2, TouchEvent, TouchPhase,
    },
    interaction::{
        ActiveInteraction, BeginContext, InputOwner, InteractionContext, InteractionEvaluator,
        InteractionOutput, InteractionResult, InteractionState,
    },
    judge::{
        JudgeEngine, JudgeError, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeStage, JudgeWindow,
        MissReason, Rule,
    },
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};
use std::error::Error;

#[derive(Clone, Copy, Debug)]
pub enum Policy {
    Locked,
    AfterRelease { grace: Duration },
}

struct ContactSustain(Policy);
impl InteractionEvaluator for ContactSustain {
    fn validate(&self, object: &TimedObject, _: &JudgeProfile) -> Result<(), JudgeError> {
        if object.time.end.is_none_or(|end| end <= object.time.start)
            || matches!(self.0, Policy::AfterRelease { grace } if grace <= Duration::ZERO)
        {
            return Err(JudgeError::InvalidObjectRange { object: object.id });
        }
        Ok(())
    }

    fn begin(
        &self,
        object: &TimedObject,
        context: &BeginContext<'_>,
    ) -> Box<dyn ActiveInteraction> {
        Box::new(Sustain {
            policy: self.0,
            start: object.time.start,
            end: object.time.end.expect("validated range"),
            control: context.control,
            state: InteractionState::Pending,
            owner: None,
            contact: None,
            released_at: None,
        })
    }
}

#[derive(Clone)]
struct Sustain {
    policy: Policy,
    start: Timestamp,
    end: Timestamp,
    control: GameControlId,
    state: InteractionState,
    owner: Option<InputOwner>,
    contact: Option<ContactId>,
    released_at: Option<Timestamp>,
}

fn inside(touch: &TouchEvent) -> bool {
    touch.position.x.is_finite()
        && touch.position.y.is_finite()
        && (0.0..=1.0).contains(&touch.position.x)
        && (0.0..=1.0).contains(&touch.position.y)
}

impl Sustain {
    fn owner_of(event: &GameInputEvent, touch: &TouchEvent) -> InputOwner {
        InputOwner {
            source: touch.meta.source,
            physical: touch.control,
            game_control: event.game_control,
        }
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

impl ActiveInteraction for Sustain {
    fn snapshot_clone(&self) -> Option<Box<dyn ActiveInteraction>> {
        Some(Box::new(self.clone()))
    }

    fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        // Canonical game-owned schema: no Debug text, pointers, or native clocks.
        let mut bytes = b"example-contact-sustain/v1:".to_vec();
        bytes.extend_from_slice(&self.start.as_nanos().to_le_bytes());
        bytes.extend_from_slice(&self.end.as_nanos().to_le_bytes());
        bytes.extend_from_slice(&self.control.0.to_le_bytes());
        match self.policy {
            Policy::Locked => bytes.push(0),
            Policy::AfterRelease { grace } => {
                bytes.push(1);
                bytes.extend_from_slice(&grace.as_nanos().to_le_bytes());
            }
        }
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
        match self.contact {
            None => bytes.push(0),
            Some(contact) => {
                bytes.push(1);
                bytes.extend_from_slice(&contact.0.to_le_bytes());
            }
        }
        match self.released_at {
            None => bytes.push(0),
            Some(at) => {
                bytes.push(1);
                bytes.extend_from_slice(&at.as_nanos().to_le_bytes());
            }
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
        let PhysicalInputEvent::Touch(touch) = &event.physical else {
            return false;
        };
        match self.state {
            InteractionState::Pending => {
                touch.phase == TouchPhase::Down
                    && inside(touch)
                    && context
                        .profile
                        .grade(
                            i128::from(context.song_time.as_nanos())
                                - i128::from(self.start.as_nanos()),
                        )
                        .is_some()
            }
            InteractionState::Active => {
                if self.owner != Some(Self::owner_of(event, touch)) {
                    return false;
                }
                match self.contact {
                    Some(contact) => touch.contact == contact && touch.phase != TouchPhase::Down,
                    None => {
                        touch.phase == TouchPhase::Down
                            && inside(touch)
                            && self
                                .deadline(context.profile)
                                .is_some_and(|at| i128::from(context.song_time.as_nanos()) <= at)
                    }
                }
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
        let PhysicalInputEvent::Touch(touch) = &event.physical else {
            return InteractionOutput::default();
        };
        if self.state == InteractionState::Pending {
            self.state = InteractionState::Active;
            self.owner = Some(Self::owner_of(event, touch));
            self.contact = Some(touch.contact);
            return InteractionOutput::default();
        }
        if self.contact.is_none() {
            self.contact = Some(touch.contact);
            self.released_at = None;
            return InteractionOutput::default();
        }
        if touch.phase == TouchPhase::Cancel || !inside(touch) {
            return self.miss(MissReason::RejectedInput);
        }
        if touch.phase != TouchPhase::Up {
            return InteractionOutput::default();
        }
        let delta = i128::from(context.song_time.as_nanos()) - i128::from(self.end.as_nanos());
        if delta >= -i128::from(context.profile.max_early().as_nanos()) {
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
        match self.policy {
            Policy::Locked => self.miss(MissReason::EarlyRelease),
            Policy::AfterRelease { .. } => {
                self.contact = None;
                self.released_at = Some(context.song_time);
                InteractionOutput::default()
            }
        }
    }

    fn advance_to(
        &mut self,
        song_time: Timestamp,
        context: &InteractionContext<'_>,
    ) -> InteractionOutput {
        if self
            .deadline(context.profile)
            .is_some_and(|at| i128::from(song_time.as_nanos()) > at)
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
            InteractionState::Completed => None,
            InteractionState::Pending => {
                Some(i128::from(self.start.as_nanos()) + i128::from(profile.max_late().as_nanos()))
            }
            InteractionState::Active => {
                let tail =
                    i128::from(self.end.as_nanos()) + i128::from(profile.max_late().as_nanos());
                let detached = match (self.policy, self.released_at) {
                    (Policy::AfterRelease { grace }, Some(at)) => {
                        i128::from(at.as_nanos()) + i128::from(grace.as_nanos())
                    }
                    _ => tail,
                };
                Some(tail.min(detached))
            }
        }
    }
}

pub fn build_engine(policy: Policy) -> Result<JudgeEngine, Box<dyn Error>> {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1)?)?;
    source.objects.push(SourceObject {
        id: ObjectId(1),
        start: Beat::new(100)?,
        end: Some(Beat::new(300)?),
        interaction: InteractionId(1),
        visual: VisualId(1),
        audio: None,
        metadata: ObjectMetadata::default(),
    });
    Ok(JudgeEngine::new(
        source.compile()?,
        vec![Rule {
            interaction: InteractionId(1),
            control: GameControlId(1),
            evaluator: Box::new(ContactSustain(policy)),
        }],
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::from_nanos(10),
                late: Duration::from_nanos(10),
            }],
            Duration::ZERO,
        )?,
    )?)
}

pub fn contact(
    device: u64,
    surface: u32,
    id: u64,
    nanos: i64,
    phase: TouchPhase,
) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(1),
        physical: PhysicalInputEvent::Touch(TouchEvent {
            meta: EventMeta::new(
                DeviceId(device),
                ClockPoint {
                    domain: ClockDomainId(7),
                    timestamp: Timestamp::from_nanos(nanos),
                },
                nanos.unsigned_abs(),
            ),
            control: PhysicalControlId::Native {
                backend: BackendId(9),
                code: surface,
            },
            contact: ContactId(id),
            phase,
            position: Position2 { x: 0.5, y: 0.5 },
            pressure: None,
        }),
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    for policy in [
        Policy::Locked,
        Policy::AfterRelease {
            grace: Duration::from_nanos(40),
        },
    ] {
        let mut engine = build_engine(policy)?;
        let events = match policy {
            Policy::Locked => vec![
                contact(20, 2, 101, 100, TouchPhase::Down),
                contact(20, 2, 101, 300, TouchPhase::Up),
            ],
            Policy::AfterRelease { .. } => vec![
                contact(20, 2, 101, 100, TouchPhase::Down),
                contact(20, 2, 101, 150, TouchPhase::Up),
                contact(20, 2, 102, 180, TouchPhase::Down),
                contact(20, 2, 102, 300, TouchPhase::Up),
            ],
        };
        for event in events {
            for result in engine.push_input(&event, event.physical.meta().timestamp)? {
                println!("{policy:?}: {result:?}");
            }
        }
        println!("state_hash={:016x}", engine.stable_hash()?);
    }
    Ok(())
}
