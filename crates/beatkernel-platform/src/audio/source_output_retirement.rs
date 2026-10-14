//! Shared retirement permission for a complete native Source output owner.
use super::NativeOutputState;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceRetirementPolicy {
    PreservePausedSuffix,
    RequireEmptySuffix,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceRetirementStatus {
    Pending,
    Refused,
    Cancelled,
    Authorized,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceRetirementError {
    InvalidTicket,
    NoncesExhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceRetirementDecision {
    Continue,
    Retire,
    Quarantined,
}

// Policy, response and nonce share one word: cancellation and permission must
// compete in the same CAS, including when the owner cached an old response.
const NONCE_SHIFT: u32 = 3;
const MAX_NONCE: u64 = u64::MAX >> NONCE_SHIFT;
const STATE_MASK: u64 = (1 << NONCE_SHIFT) - 1;
const IDLE: u64 = 0;
const PENDING_PRESERVE: u64 = 1;
const PENDING_EMPTY: u64 = 2;
const REFUSED: u64 = 3;
const CANCELLED: u64 = 4;
const AUTHORIZED: u64 = 5;

pub(crate) struct SourceRetirementSlot {
    word: AtomicU64,
}

impl SourceRetirementSlot {
    pub(crate) const fn new() -> Self {
        Self {
            word: AtomicU64::new(IDLE),
        }
    }

    pub(crate) fn submit(
        &self,
        policy: SourceRetirementPolicy,
    ) -> Result<Option<u64>, SourceRetirementError> {
        self.submit_prepared(policy, || {})
    }

    /// Prepares a payload before publishing its pending request to the owner.
    /// The caller must ensure one payload producer; `prepare` must run
    /// synchronously and only write atomics. Busy or exhausted slots never call it.
    pub(crate) fn submit_prepared(
        &self,
        policy: SourceRetirementPolicy,
        prepare: impl FnOnce(),
    ) -> Result<Option<u64>, SourceRetirementError> {
        let previous = self.word.load(Ordering::Acquire);
        if previous & STATE_MASK != IDLE {
            return Ok(None);
        }
        let nonce = (previous >> NONCE_SHIFT)
            .checked_add(1)
            .filter(|nonce| *nonce <= MAX_NONCE)
            .ok_or(SourceRetirementError::NoncesExhausted)?;
        let pending = match policy {
            SourceRetirementPolicy::PreservePausedSuffix => PENDING_PRESERVE,
            SourceRetirementPolicy::RequireEmptySuffix => PENDING_EMPTY,
        };
        prepare();
        Ok(self
            .word
            .compare_exchange(
                previous,
                (nonce << NONCE_SHIFT) | pending,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .ok()
            .map(|_| nonce))
    }

    fn matching_word(&self, nonce: u64) -> Result<u64, SourceRetirementError> {
        let word = self.word.load(Ordering::Acquire);
        if nonce == 0 || word >> NONCE_SHIFT != nonce || word & STATE_MASK == IDLE {
            return Err(SourceRetirementError::InvalidTicket);
        }
        Ok(word)
    }

    fn decode(word: u64) -> SourceRetirementStatus {
        match word & STATE_MASK {
            PENDING_PRESERVE | PENDING_EMPTY => SourceRetirementStatus::Pending,
            REFUSED => SourceRetirementStatus::Refused,
            CANCELLED => SourceRetirementStatus::Cancelled,
            AUTHORIZED => SourceRetirementStatus::Authorized,
            _ => unreachable!("matching retirement word contains a request or response"),
        }
    }

    pub(crate) fn status(
        &self,
        nonce: u64,
    ) -> Result<SourceRetirementStatus, SourceRetirementError> {
        self.matching_word(nonce).map(Self::decode)
    }

    pub(crate) fn poll(&self, nonce: u64) -> Result<SourceRetirementStatus, SourceRetirementError> {
        let word = self.matching_word(nonce)?;
        let status = Self::decode(word);
        if matches!(
            status,
            SourceRetirementStatus::Refused | SourceRetirementStatus::Cancelled
        ) {
            self.word
                .compare_exchange(
                    word,
                    nonce << NONCE_SHIFT,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .map_err(|_| SourceRetirementError::InvalidTicket)?;
        }
        Ok(status)
    }

    pub(crate) fn cancel(
        &self,
        nonce: u64,
    ) -> Result<SourceRetirementStatus, SourceRetirementError> {
        let word = self.matching_word(nonce)?;
        if Self::decode(word) != SourceRetirementStatus::Pending {
            return Ok(Self::decode(word));
        }
        match self.word.compare_exchange(
            word,
            (nonce << NONCE_SHIFT) | CANCELLED,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Ok(SourceRetirementStatus::Cancelled),
            Err(_) => self.status(nonce),
        }
    }

    pub(crate) fn decision(&self, state: &NativeOutputState) -> SourceRetirementDecision {
        self.decision_with(state, || true)
    }

    /// Checks destination eligibility after acquiring an admissible pending request.
    /// The predicate may read its atomic payload; authorization still compares
    /// that exact request word if cancellation or another nonce races the read.
    pub(crate) fn decision_with(
        &self,
        state: &NativeOutputState,
        eligible: impl FnOnce() -> bool,
    ) -> SourceRetirementDecision {
        let word = self.word.load(Ordering::Acquire);
        if word & STATE_MASK == AUTHORIZED {
            return SourceRetirementDecision::Retire;
        }
        if state.admission_is_uncertain() {
            return SourceRetirementDecision::Quarantined;
        }
        let permitted = match word & STATE_MASK {
            PENDING_PRESERVE => state.pending_frames() == 0 || state.paused_tail_admissible(),
            PENDING_EMPTY => state.pending_frames() == 0,
            _ => return SourceRetirementDecision::Continue,
        } && eligible();
        let response = if permitted { AUTHORIZED } else { REFUSED };
        match self.word.compare_exchange(
            word,
            (word & !STATE_MASK) | response,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) if permitted => SourceRetirementDecision::Retire,
            Err(current) if current & STATE_MASK == AUTHORIZED => SourceRetirementDecision::Retire,
            _ => SourceRetirementDecision::Continue,
        }
    }
}

#[cfg(test)]
#[path = "source_output_retirement_fixtures.rs"]
pub(crate) mod fixtures;
