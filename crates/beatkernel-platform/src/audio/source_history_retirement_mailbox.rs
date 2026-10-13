//! Bounded Source history intent, separate from native presentation authority.
use beatkernel::audio::{AudioError, Mixer, OutputFrameBasis, SourcePlaybackHistory};
use std::sync::atomic::{AtomicU64, Ordering};

/// A mathematical minimum queued for the owning callback, not a native ACK.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SourceHistoryRetirementRequest {
    pub(crate) revision: u64,
    pub(crate) source_pin: u64,
}

/// Applies a qualified mathematical request on the actual Mixer owner. Live
/// practice receipts can keep an older interval alive; that request remains
/// pending until those receipts have been consumed.
pub(crate) fn apply_source_history_retirement(
    mixer: &mut Mixer,
    request: SourceHistoryRetirementRequest,
    completed: &mut u64,
) -> Result<usize, AudioError> {
    if request.revision == 0 || request.revision & 1 != 0 {
        return Err(AudioError::InvalidFormat);
    }
    if request.revision <= *completed {
        return Ok(0);
    }
    // Validate the requested range even if a practice receipt would clamp it.
    mixer
        .source_playback_history()
        .ok_or(AudioError::InvalidCapacity)?
        .validate_retirement_before(request.source_pin)?;
    let pin = mixer
        .practice_history_retention_pins()
        .0
        .map_or(request.source_pin, |practice| {
            practice.min(request.source_pin)
        });
    let retired = mixer.retire_source_playback_before(pin)?;
    if pin == request.source_pin {
        *completed = request.revision;
    }
    Ok(retired)
}

/// Inline storage bound to the original stream and Source history identity.
/// Native callers must qualify current generated/publication bounds before
/// requesting retirement. This mailbox neither renders nor retires history.
pub(crate) struct SourceHistoryRetirementMailbox {
    basis: OutputFrameBasis,
    source_identity: (u32, u64, u64),
    source_lower: u64,
    // Zero is empty; odd reserves the payload for one producer.
    sequence: AtomicU64,
    source_pin: AtomicU64,
}

impl SourceHistoryRetirementMailbox {
    pub(crate) fn new(
        basis: OutputFrameBasis,
        history: &SourcePlaybackHistory,
    ) -> Result<Self, AudioError> {
        if basis.sample_rate() != history.source_rate()
            || history.initial_physical_frame() > basis.start_physical_frame()
        {
            return Err(AudioError::InvalidFormat);
        }
        Ok(Self {
            basis,
            source_identity: (
                history.source_rate(),
                history.initial_physical_frame(),
                history.initial_playback_frame(),
            ),
            source_lower: history.retained_start_frame(),
            sequence: AtomicU64::new(0),
            source_pin: AtomicU64::new(0),
        })
    }

    pub(crate) fn validate_identity(
        &self,
        expected_basis: OutputFrameBasis,
        source_identity: (u32, u64, u64),
    ) -> Result<(), AudioError> {
        if expected_basis != self.basis || source_identity != self.source_identity {
            return Err(AudioError::InvalidFormat);
        }
        Ok(())
    }

    /// Queues a caller-qualified minimum in at most three attempts. A returned
    /// revision identifies a request, never callback completion or permission.
    pub(crate) fn request(
        &self,
        expected_basis: OutputFrameBasis,
        source_identity: (u32, u64, u64),
        source_pin: u64,
    ) -> Result<Option<u64>, AudioError> {
        self.validate_identity(expected_basis, source_identity)?;
        if source_pin < self.source_lower {
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
                if source_pin < previous.source_pin {
                    return Err(AudioError::InvalidFormat);
                }
                if source_pin == previous.source_pin {
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
            self.source_pin.store(source_pin, Ordering::SeqCst);
            self.sequence.store(next, Ordering::SeqCst);
            return Ok(Some(next));
        }
        Ok(None)
    }

    /// Bounded coherent read. Busy or malformed publications expose no intent.
    pub(crate) fn latest(&self) -> Option<SourceHistoryRetirementRequest> {
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
    ) -> Result<Option<SourceHistoryRetirementRequest>, AudioError> {
        let source_pin = self.source_pin.load(Ordering::SeqCst);
        if self.sequence.load(Ordering::SeqCst) != sequence {
            return Ok(None);
        }
        if source_pin < self.source_lower {
            return Err(AudioError::InvalidFormat);
        }
        Ok(Some(SourceHistoryRetirementRequest {
            revision: sequence,
            source_pin,
        }))
    }
}

#[cfg(test)]
#[path = "source_history_retirement_mailbox_fixtures.rs"]
mod source_history_retirement_mailbox_fixtures;

#[cfg(test)]
#[path = "source_history_retirement_apply_fixtures.rs"]
mod source_history_retirement_apply_fixtures;
