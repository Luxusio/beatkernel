//! The converted worker's single decision before generation or native admission.
use super::{AlsaRequest, LinuxError, PumpAction, Shared};
use crate::audio::{ConvertedNativeOutputState, DeviceFormat};
use beatkernel::{
    audio::{AudioError, AudioFormat, ChannelMatrix, FormatConverter, ResampleQuality},
    time::ClockDomainId,
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Weak,
};

/// How the worker may retain positively unsubmitted converted output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConvertedRetirementPolicy {
    /// Only an unchanged interpretation may preserve a nonempty held suffix.
    PreserveHeldSuffix,
    /// A changed interpretation requires no positively unsubmitted output.
    RequireEmptySuffix,
}
impl ConvertedRetirementPolicy {
    /// Selects preservation only for the exact applied format and channel matrix.
    pub fn for_interpretation(
        applied: DeviceFormat,
        current_matrix: &ChannelMatrix,
        requested: DeviceFormat,
        requested_matrix: &ChannelMatrix,
    ) -> Self {
        if applied == requested && current_matrix == requested_matrix {
            Self::PreserveHeldSuffix
        } else {
            Self::RequireEmptySuffix
        }
    }
}

/// A request bound to this stream's existing allocation and checked nonce.
/// The weak owner keeps a stale ticket from aliasing a reused stream allocation.
#[derive(Clone, Debug)]
pub struct ConvertedRetirementTicket {
    nonce: u64,
    owner: Weak<Shared>,
}
impl ConvertedRetirementTicket {
    pub(super) fn new(owner: &Arc<Shared>, nonce: u64) -> Self {
        Self {
            nonce,
            owner: Arc::downgrade(owner),
        }
    }
    pub(super) fn nonce_for(&self, owner: &Arc<Shared>) -> Result<u64, LinuxError> {
        if !self.owner.ptr_eq(&Arc::downgrade(owner)) {
            return Err(LinuxError::InvalidLifecycle);
        }
        Ok(self.nonce)
    }
}

/// Conditional authorization is distinct from completed native drop and join.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConvertedRetirementStatus {
    /// The sole worker has not yet decided against its actual pending state.
    Pending,
    /// The worker refused and continues the same stream without reinterpretation.
    Refused,
    /// Matching cancellation won before the worker authorized retirement.
    Cancelled,
    /// No further generation or write is permitted; native cleanup may remain.
    Authorized,
}

const NONCE_SHIFT: u32 = 3;
const MAX_NONCE: u64 = u64::MAX >> NONCE_SHIFT;
const STATE_MASK: u64 = (1 << NONCE_SHIFT) - 1;
const IDLE: u64 = 0;
const PENDING_PRESERVE: u64 = 1;
const PENDING_EMPTY: u64 = 2;
const REFUSED: u64 = 3;
const CANCELLED: u64 = 4;
const AUTHORIZED: u64 = 5;

pub(super) struct ConditionalRetirement {
    word: AtomicU64,
}
impl ConditionalRetirement {
    pub(super) const fn new() -> Self {
        Self {
            word: AtomicU64::new(IDLE),
        }
    }
    pub(super) fn submit(
        &self,
        policy: ConvertedRetirementPolicy,
    ) -> Result<Option<u64>, LinuxError> {
        let previous = self.word.load(Ordering::Acquire);
        if previous & STATE_MASK != IDLE {
            return Ok(None);
        }
        let nonce = (previous >> NONCE_SHIFT)
            .checked_add(1)
            .filter(|nonce| *nonce <= MAX_NONCE)
            .ok_or(LinuxError::Overflow)?;
        let policy = match policy {
            ConvertedRetirementPolicy::PreserveHeldSuffix => PENDING_PRESERVE,
            ConvertedRetirementPolicy::RequireEmptySuffix => PENDING_EMPTY,
        };
        Ok(self
            .word
            .compare_exchange(
                previous,
                (nonce << NONCE_SHIFT) | policy,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .ok()
            .map(|_| nonce))
    }
    fn matching_word(&self, nonce: u64) -> Result<u64, LinuxError> {
        let word = self.word.load(Ordering::Acquire);
        if nonce == 0 || word >> NONCE_SHIFT != nonce || word & STATE_MASK == IDLE {
            return Err(LinuxError::InvalidLifecycle);
        }
        Ok(word)
    }
    fn decode(word: u64) -> ConvertedRetirementStatus {
        match word & STATE_MASK {
            PENDING_PRESERVE | PENDING_EMPTY => ConvertedRetirementStatus::Pending,
            REFUSED => ConvertedRetirementStatus::Refused,
            CANCELLED => ConvertedRetirementStatus::Cancelled,
            AUTHORIZED => ConvertedRetirementStatus::Authorized,
            _ => unreachable!("matching retirement word contains a request or response"),
        }
    }
    pub(super) fn status(&self, nonce: u64) -> Result<ConvertedRetirementStatus, LinuxError> {
        self.matching_word(nonce).map(Self::decode)
    }
    pub(super) fn poll(&self, nonce: u64) -> Result<ConvertedRetirementStatus, LinuxError> {
        let word = self.matching_word(nonce)?;
        let status = Self::decode(word);
        if matches!(
            status,
            ConvertedRetirementStatus::Refused | ConvertedRetirementStatus::Cancelled
        ) {
            self.word
                .compare_exchange(
                    word,
                    nonce << NONCE_SHIFT,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .map_err(|_| LinuxError::InvalidLifecycle)?;
        }
        Ok(status)
    }
    pub(super) fn cancel(&self, nonce: u64) -> Result<ConvertedRetirementStatus, LinuxError> {
        let word = self.matching_word(nonce)?;
        if Self::decode(word) != ConvertedRetirementStatus::Pending {
            return Ok(Self::decode(word));
        }
        match self.word.compare_exchange(
            word,
            (nonce << NONCE_SHIFT) | CANCELLED,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Ok(ConvertedRetirementStatus::Cancelled),
            Err(_) => self.status(nonce),
        }
    }
    pub(super) fn worker_decision(&self, state: &ConvertedNativeOutputState) -> PumpAction {
        let word = self.word.load(Ordering::Acquire);
        let permitted = match word & STATE_MASK {
            AUTHORIZED => return PumpAction::Retire,
            PENDING_PRESERVE => state.pending_frames() == 0 || state.pending_is_held(),
            PENDING_EMPTY => state.pending_frames() == 0,
            _ => return PumpAction::Continue,
        };
        let response = if permitted { AUTHORIZED } else { REFUSED };
        match self.word.compare_exchange(
            word,
            (word & !STATE_MASK) | response,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) if permitted => PumpAction::Retire,
            Err(current) if current & STATE_MASK == AUTHORIZED => PumpAction::Retire,
            _ => PumpAction::Continue,
        }
    }
}

/// Immutable source facts suffice for cold capacity validation without ownership.
#[derive(Clone, Copy)]
pub(super) struct ConvertedSourceFacts {
    source: AudioFormat,
    quality: ResampleQuality,
    render_limit: usize,
    domain: ClockDomainId,
}
impl ConvertedSourceFacts {
    pub(super) fn from_state(state: &ConvertedNativeOutputState) -> Self {
        let config = state.mixer().config();
        Self {
            source: config.format(),
            quality: state.converter_owner().converter().quality(),
            render_limit: config.limits().max_render_frames(),
            domain: config.domain(),
        }
    }
    pub(super) fn validate(
        self,
        request: &AlsaRequest,
        matrix: &ChannelMatrix,
    ) -> Result<(), LinuxError> {
        if request.device.is_empty()
            || request.device.contains('\0')
            || request.period_frames == 0
            || request.buffer_frames <= request.period_frames
            || request.format.channel_mask().is_some()
            || request.monotonic_domain == self.domain
        {
            return Err(LinuxError::InvalidConfiguration(
                "converted ALSA requires an explicit endpoint, distinct clocks, unspecified mask and 0 < period < buffer",
            ));
        }
        super::native_format_name(request.format.encoding())?;
        if matrix.source_channels() != self.source.channels()
            || matrix.target_channels() != request.format.channels()
        {
            return Err(LinuxError::Mixer(AudioError::InvalidFormat));
        }
        let required = FormatConverter::required_source_frames_continuous(
            self.source,
            request.format.pcm(),
            self.quality,
            request.period_frames as usize,
        )
        .map_err(LinuxError::Mixer)?;
        if required > self.render_limit {
            return Err(LinuxError::Mixer(AudioError::RenderCapacity));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "converted_conditional_retirement_fixtures.rs"]
mod fixtures;
