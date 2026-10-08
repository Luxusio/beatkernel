//! Transient facts from an accepted judge input transition.

use crate::chart::ObjectId;

use super::JudgeEvent;

/// Input ownership classification before the accepted transition commits it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputFreshness {
    /// A new enabled button or contact Down.
    FreshPress,
    /// A Down already owned by this source and physical/logical destination.
    HeldDown,
    /// A button Repeat, regardless of whether the button is owned.
    ExplicitRepeat,
    /// Other input, including a Down with contact semantics disabled.
    Other,
}

/// Authoritative admission and phase facts, independent of scoring policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputDisposition {
    freshness: InputFreshness,
    candidate_count: usize,
    selected: Option<ObjectId>,
    dispatched_count: usize,
    input_result_count: usize,
    passive_result_count: usize,
    input_hazard_count: usize,
}

impl InputDisposition {
    /// Ownership classification computed before this input commits ownership.
    pub const fn freshness(&self) -> InputFreshness {
        self.freshness
    }

    /// Number of actual eligible candidates before resolver selection.
    pub const fn candidate_count(&self) -> usize {
        self.candidate_count
    }

    /// Valid resolver selection, which alone does not establish dispatch.
    pub const fn selected(&self) -> Option<ObjectId> {
        self.selected
    }

    /// Number of actual on_input callbacks after lifecycle/admission filters.
    pub const fn dispatched_count(&self) -> usize {
        self.dispatched_count
    }

    /// Number of stamped results emitted by input callbacks.
    pub const fn input_result_count(&self) -> usize {
        self.input_result_count
    }

    /// Number of stamped results emitted by expiry before input callbacks.
    pub const fn passive_result_count(&self) -> usize {
        self.passive_result_count
    }

    /// Newly consumed inclusive-phase hazard markers, across all controls and
    /// outcomes. Earlier passive-phase markers are excluded.
    pub const fn input_hazard_count(&self) -> usize {
        self.input_hazard_count
    }

    /// Whether a fresh press had no candidates, callbacks or input results.
    /// Passive results and hazards remain independent facts.
    pub const fn unmatched_fresh_press(&self) -> bool {
        matches!(self.freshness, InputFreshness::FreshPress)
            && self.candidate_count == 0
            && self.dispatched_count == 0
            && self.input_result_count == 0
    }
}

/// Original judge events and transient facts for one accepted input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JudgeInputReport {
    /// Original ordered results from expiry and input callbacks.
    pub events: Vec<JudgeEvent>,
    /// Facts from the same accepted transition.
    pub disposition: InputDisposition,
}

// Exactly two private static modes keep the legacy return ABI and avoid
// constructing a rich report or collecting reporting-only facts on that path.
pub(super) trait InputSink: Default {
    type Output;
    const OBSERVED: bool;

    fn admitted(&mut self, _: InputFreshness, _: usize, _: Option<ObjectId>) {}
    fn passive_results(&mut self, _: usize) {}
    fn dispatched(&mut self) {}
    fn input_hazards(&mut self, _: usize) {}
    fn finish(self, events: Vec<JudgeEvent>) -> Self::Output;
}

#[derive(Default)]
pub(super) struct LegacyInput;

impl InputSink for LegacyInput {
    type Output = Vec<JudgeEvent>;
    const OBSERVED: bool = false;

    fn finish(self, events: Vec<JudgeEvent>) -> Self::Output {
        events
    }
}

pub(super) struct ObservedInput(InputDisposition);

impl Default for ObservedInput {
    fn default() -> Self {
        Self(InputDisposition {
            freshness: InputFreshness::Other,
            candidate_count: 0,
            selected: None,
            dispatched_count: 0,
            input_result_count: 0,
            passive_result_count: 0,
            input_hazard_count: 0,
        })
    }
}

impl InputSink for ObservedInput {
    type Output = JudgeInputReport;
    const OBSERVED: bool = true;

    fn admitted(&mut self, freshness: InputFreshness, count: usize, selected: Option<ObjectId>) {
        self.0.freshness = freshness;
        self.0.candidate_count = count;
        self.0.selected = selected;
    }

    fn passive_results(&mut self, count: usize) {
        self.0.passive_result_count = count;
    }

    fn dispatched(&mut self) {
        self.0.dispatched_count += 1;
    }

    fn input_hazards(&mut self, count: usize) {
        self.0.input_hazard_count = count;
    }

    fn finish(mut self, events: Vec<JudgeEvent>) -> Self::Output {
        // Only stamped input callback results are appended after expiry.
        self.0.input_result_count = events.len() - self.0.passive_result_count;
        JudgeInputReport {
            events,
            disposition: self.0,
        }
    }
}
