//! Exact BMS damage evidence from committed one-shot core hazard outcomes.

use beatkernel::judge::{HazardEvent, HazardOutcome};
use std::fmt;

/// Invalid opaque damage or an unrepresentable accumulated counter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MineDamageError {
    /// BMS damage must be a nonzero base36 value, including the fatal sentinel.
    InvalidDamage { value: u64 },
    /// A full-width count or nonfatal damage total overflowed.
    CounterOverflow,
}

impl fmt::Display for MineDamageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BMS mine damage: {self:?}")
    }
}
impl std::error::Error for MineDamageError {}

/// Committed mine outcomes, separate from normal-note scores and gauge policy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MineDamageSummary {
    /// Number of occupied markers, including fatal markers.
    pub triggered: u64,
    /// Number of unoccupied markers.
    pub avoided: u64,
    /// Exact accumulated nonfatal damage in half-percent units.
    pub half_percent_damage: u64,
    /// Whether a triggered ZZ marker has ever been observed.
    pub instant_death: bool,
}

impl MineDamageSummary {
    /// Consumes each actual committed event once. The entire batch is atomic;
    /// this summary neither deduplicates identities nor applies playback policy.
    pub fn observe(&mut self, events: &[HazardEvent]) -> Result<(), MineDamageError> {
        let mut next = *self;
        for event in events {
            if !(1..=1295).contains(&event.value) {
                return Err(MineDamageError::InvalidDamage { value: event.value });
            }
            match event.outcome {
                HazardOutcome::Avoided => {
                    next.avoided = next
                        .avoided
                        .checked_add(1)
                        .ok_or(MineDamageError::CounterOverflow)?;
                }
                HazardOutcome::Triggered => {
                    next.triggered = next
                        .triggered
                        .checked_add(1)
                        .ok_or(MineDamageError::CounterOverflow)?;
                    if event.value == 1295 {
                        next.instant_death = true;
                    } else {
                        next.half_percent_damage = next
                            .half_percent_damage
                            .checked_add(event.value)
                            .ok_or(MineDamageError::CounterOverflow)?;
                    }
                }
            }
        }
        *self = next;
        Ok(())
    }
}
