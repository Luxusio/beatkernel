//! Bounded producer-to-callback retirement requests, not native acknowledgments.
use beatkernel::audio::{
    AudioError, ConvertedPhaseHistory, SourcePlaybackHistory, TargetFrameBasis, TargetTime,
};
use std::cmp::Ordering as Comparison;
use std::sync::atomic::{AtomicU64, Ordering};

/// One coherent, coalesced mathematical request for the owning callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ConvertedHistoryRetirementRequest {
    pub(crate) revision: u64,
    pub(crate) source_pin: u64,
    pub(crate) target_pin: TargetTime,
}

/// Inline storage bound to one stream and the original source history identity.
/// The sequence protects only the request payload across producer/callback
/// threads. Native timing and history publication retain their own authority.
pub(crate) struct ConvertedHistoryRetirementMailbox {
    basis: TargetFrameBasis,
    source_identity: (u32, u64, u64),
    source_lower: u64,
    target_lower: TargetTime,
    // Zero means empty; odd means a producer owns the four payload words.
    sequence: AtomicU64,
    // Source pin, target whole seconds, proper numerator, denominator.
    words: [AtomicU64; 4],
}

impl ConvertedHistoryRetirementMailbox {
    pub(crate) fn new(
        basis: TargetFrameBasis,
        source: &SourcePlaybackHistory,
        phase: &ConvertedPhaseHistory,
    ) -> Result<Self, AudioError> {
        if source.source_rate() != phase.source_rate() {
            return Err(AudioError::InvalidFormat);
        }
        Ok(Self {
            basis,
            source_identity: (
                source.source_rate(),
                source.initial_physical_frame(),
                source.initial_playback_frame(),
            ),
            source_lower: source.retained_start_frame(),
            target_lower: phase.retained_start_time(),
            sequence: AtomicU64::new(0),
            words: [0, 0, 0, 1].map(AtomicU64::new),
        })
    }

    pub(crate) fn validate_identity(
        &self,
        expected_basis: TargetFrameBasis,
        source_identity: (u32, u64, u64),
    ) -> Result<(), AudioError> {
        if expected_basis != self.basis || source_identity != self.source_identity {
            return Err(AudioError::InvalidFormat);
        }
        Ok(())
    }

    /// Queues a caller-qualified minimum. A revision is not completion or a
    /// native pause acknowledgment. Current generated bounds are qualified by
    /// the caller's existing history publication before invoking this method.
    pub(crate) fn request(
        &self,
        expected_basis: TargetFrameBasis,
        source_identity: (u32, u64, u64),
        source_pin: u64,
        target_pin: TargetTime,
    ) -> Result<Option<u64>, AudioError> {
        self.validate_identity(expected_basis, source_identity)?;
        if source_pin < self.source_lower || compare_time(target_pin, self.target_lower).is_lt() {
            return Err(AudioError::InvalidFormat);
        }
        for _ in 0..3 {
            let sequence = self.sequence.load(Ordering::SeqCst);
            if sequence & 1 != 0 {
                continue;
            }
            if sequence != 0 {
                let Some(previous) = self.stable_request(sequence)? else {
                    continue;
                };
                let target_order = compare_time(target_pin, previous.target_pin);
                if source_pin < previous.source_pin || target_order.is_lt() {
                    return Err(AudioError::InvalidFormat);
                }
                if source_pin == previous.source_pin && target_order.is_eq() {
                    return Ok(Some(sequence));
                }
            }
            let next = sequence.checked_add(2).ok_or(AudioError::Overflow)?;
            if self
                .sequence
                .compare_exchange(sequence, sequence + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                continue;
            }
            for (word, value) in self.words.iter().zip([
                source_pin,
                target_pin.seconds(),
                target_pin.numerator(),
                target_pin.denominator(),
            ]) {
                word.store(value, Ordering::SeqCst);
            }
            self.sequence.store(next, Ordering::SeqCst);
            return Ok(Some(next));
        }
        Ok(None)
    }

    /// A bounded stable read of the latest nonempty request. Contention and
    /// malformed payloads yield no request; the callback never waits.
    pub(crate) fn latest(&self) -> Option<ConvertedHistoryRetirementRequest> {
        for _ in 0..3 {
            let sequence = self.sequence.load(Ordering::SeqCst);
            if sequence == 0 || sequence & 1 != 0 {
                continue;
            }
            match self.stable_request(sequence) {
                Ok(Some(request)) => return Some(request),
                Ok(None) => continue,
                Err(_) => return None,
            }
        }
        None
    }

    fn stable_request(
        &self,
        sequence: u64,
    ) -> Result<Option<ConvertedHistoryRetirementRequest>, AudioError> {
        let words: [u64; 4] = std::array::from_fn(|index| self.words[index].load(Ordering::SeqCst));
        if self.sequence.load(Ordering::SeqCst) != sequence {
            return Ok(None);
        }
        let target_pin = TargetTime::new(words[1], words[2], words[3])?;
        if words[0] < self.source_lower || compare_time(target_pin, self.target_lower).is_lt() {
            return Err(AudioError::InvalidFormat);
        }
        Ok(Some(ConvertedHistoryRetirementRequest {
            revision: sequence,
            source_pin: words[0],
            target_pin,
        }))
    }
}

fn compare_time(left: TargetTime, right: TargetTime) -> Comparison {
    left.seconds().cmp(&right.seconds()).then_with(|| {
        (u128::from(left.numerator()) * u128::from(right.denominator()))
            .cmp(&(u128::from(right.numerator()) * u128::from(left.denominator())))
    })
}

#[cfg(test)]
#[path = "history_retirement_mailbox_fixtures.rs"]
mod history_retirement_mailbox_fixtures;
