//! Infallible interaction transitions over typed bound input.
//!
//! Validation precedes engine mutation. Callback panics and external side
//! effects are outside the engine's atomic-error guarantee. Outputs contain
//! only stage results: the engine stamps object, time and input provenance.

mod advanced;
mod builtin;
pub use advanced::{CompositeEvaluator, RepeatedEvaluator, TrackingEvaluator, TrackingInput};
pub use builtin::{HoldEvaluator, InstantEvaluator};

use crate::{
    chart::TimedObject,
    input::{DeviceId, GameControlId, GameInputEvent, PhysicalControlId},
    judge::{JudgeError, JudgeOutcome, JudgePolicy, JudgeProfile, JudgeStage},
    time::Timestamp,
};

/// A physical input owner scoped to one logical bound destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InputOwner {
    /// Runtime device identity.
    pub source: DeviceId,
    /// Complete physical control identity.
    pub physical: PhysicalControlId,
    /// Logical destination, preserving independent binding fanout.
    pub game_control: GameControlId,
}

/// The current lifecycle phase of one interaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionState {
    /// Waiting for a qualifying start.
    Pending,
    /// A start succeeded and later input or time can finish the interaction.
    Active,
    /// Terminal; no further result may be emitted.
    Completed,
}

/// Configuration supplied while beginning a validated object.
pub struct BeginContext<'a> {
    /// Logical destination registered for this interaction.
    pub control: GameControlId,
    /// Validated timing configuration.
    pub profile: &'a JudgeProfile,
}

/// Effective song time and grading supplied to interaction callbacks.
pub struct InteractionContext<'a> {
    /// Mapped song time after the profile offset has been applied once.
    pub song_time: Timestamp,
    /// Validated timing configuration.
    pub profile: &'a JudgeProfile,
    /// Infallible caller-selected grading policy.
    pub policy: &'a dyn JudgePolicy,
}

/// One evaluator result, without engine-owned identity or provenance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InteractionResult {
    /// Reported stage.
    pub stage: JudgeStage,
    /// Grade or explicit miss.
    pub outcome: JudgeOutcome,
}

/// Ordered stage results of one infallible callback.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InteractionOutput {
    /// Results in callback-defined order.
    pub results: Vec<InteractionResult>,
}

/// Setup-time validation and per-object active-interaction construction.
pub trait InteractionEvaluator: Send + Sync {
    /// Declares start routing; custom evaluators own acceptance by default.
    fn start_eligibility(&self) -> StartEligibility {
        StartEligibility::EvaluatorDefined
    }

    /// Rejects malformed configuration before an engine is constructed.
    fn validate(&self, object: &TimedObject, profile: &JudgeProfile) -> Result<(), JudgeError>;

    /// Begins an object already accepted by [`Self::validate`].
    fn begin(&self, object: &TimedObject, context: &BeginContext<'_>)
        -> Box<dyn ActiveInteraction>;
}

/// Eligibility used to build separate builtin and custom pending indexes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartEligibility {
    /// A fresh button Down inside the profile's widest start window.
    ProfileButtonPress,
    /// The evaluator predicate decides eligibility for unchanged typed input.
    EvaluatorDefined,
}

/// Single-owner state transitions for one validated object.
pub trait ActiveInteraction: Send {
    /// Additional logical destinations observed by this interaction.
    ///
    /// These are immutable for the interaction lifetime and included in native
    /// engine routing, allowing prerequisite state separate from a trigger.
    fn additional_controls(&self) -> &[GameControlId] {
        &[]
    }
    /// Deep-copies all state for reusable in-memory checkpoints.
    /// Custom implementations remain source-compatible and unsupported by default.
    fn snapshot_clone(&self) -> Option<Box<dyn ActiveInteraction>> {
        None
    }

    /// Versioned canonical bytes for all state affecting future transitions.
    /// Implementations must include a stable implementation/schema identity.
    fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        None
    }

    /// Returns the current lifecycle phase.
    fn state(&self) -> InteractionState;

    /// Tests whether this unchanged typed input should be routed here.
    fn accepts_input(&self, event: &GameInputEvent, context: &InteractionContext<'_>) -> bool;

    /// Consumes a routed input, preserving terminal-result uniqueness.
    fn on_input(
        &mut self,
        event: &GameInputEvent,
        context: &InteractionContext<'_>,
    ) -> InteractionOutput;

    /// Advances to effective song time; deadlines expire strictly before it.
    ///
    /// Pending interactions are called on deadline expiry. Active interactions
    /// also receive each engine advance. Pending interactions with no deadline
    /// receive no time callbacks.
    fn advance_to(
        &mut self,
        song_time: Timestamp,
        context: &InteractionContext<'_>,
    ) -> InteractionOutput;

    /// Returns the inclusive late deadline in wide nanoseconds, or no deadline.
    ///
    /// Completed interactions return `None`. Wide arithmetic permits deadlines
    /// beyond the representable timestamp range without wrapping.
    /// After advancing strictly past a returned deadline, the implementation
    /// must consume it by completing or returning a strictly later deadline.
    fn deadline(&self, profile: &JudgeProfile) -> Option<i128>;
}
