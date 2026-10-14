//! The complete Source owner's WASAPI decision and one synchronous native fill.
//! Native effects are statically supplied by the owner-thread COM adapter.
use super::{
    channel_remix, source_history_retirement_mailbox::SourceHistoryRetirementRequest,
    AudioBackendKind, AudioPlatformError, AudioStreamMode, AudioStreamRequest, BufferRequest,
    DeviceFormat, NativeOutputState, PeriodRequest,
};
use beatkernel::audio::{AudioFormat, ChannelMatrix, RenderReport};
use std::sync::atomic::{AtomicU64, Ordering};

const INVALID_ARGUMENT: i32 = 0x8007_0057u32 as i32;

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

    pub(crate) fn submit_validated(
        &self,
        facts: &SourceRetirementFacts,
        request: &AudioStreamRequest,
        matrix: Option<&ChannelMatrix>,
        max_frames: usize,
    ) -> Result<Option<u64>, AudioPlatformError> {
        let policy = facts.validate(request, matrix, max_frames)?;
        self.submit(policy).map_err(|error| match error {
            SourceRetirementError::InvalidTicket => AudioPlatformError::InvalidRequest,
            SourceRetirementError::NoncesExhausted => AudioPlatformError::Capacity,
        })
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

    pub(crate) fn worker_decision(&self, state: &NativeOutputState) -> WasapiSourceStep {
        let word = self.word.load(Ordering::Acquire);
        if word & STATE_MASK == AUTHORIZED {
            return WasapiSourceStep::Retire;
        }
        if state.admission_is_uncertain() {
            return failure(WasapiSourcePhase::Quarantined, INVALID_ARGUMENT);
        }
        let permitted = match word & STATE_MASK {
            PENDING_PRESERVE => state.pending_frames() == 0 || state.paused_tail_admissible(),
            PENDING_EMPTY => state.pending_frames() == 0,
            _ => return WasapiSourceStep::Continue,
        };
        let response = if permitted { AUTHORIZED } else { REFUSED };
        match self.word.compare_exchange(
            word,
            (word & !STATE_MASK) | response,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) if permitted => WasapiSourceStep::Retire,
            Err(current) if current & STATE_MASK == AUTHORIZED => WasapiSourceStep::Retire,
            _ => WasapiSourceStep::Continue,
        }
    }
}

/// Copied once outside rendering; no observed pending count becomes permission.
pub(crate) struct SourceRetirementFacts {
    source: AudioFormat,
    applied: DeviceFormat,
    matrix: Option<ChannelMatrix>,
    render_limit: usize,
}

impl SourceRetirementFacts {
    pub(crate) fn from_state(
        state: &NativeOutputState,
        current_matrix: Option<ChannelMatrix>,
    ) -> Result<Self, AudioPlatformError> {
        let applied = state.format().ok_or(AudioPlatformError::InvalidFormat)?;
        state
            .validate_reconfigure(applied, current_matrix.as_ref(), state.max_frames())
            .map_err(|_| AudioPlatformError::InvalidFormat)?;
        Ok(Self {
            source: state.mixer().config().format(),
            applied,
            matrix: current_matrix,
            render_limit: state.mixer().config().limits().max_render_frames(),
        })
    }

    pub(crate) fn validate(
        &self,
        request: &AudioStreamRequest,
        requested_matrix: Option<&ChannelMatrix>,
        max_frames: usize,
    ) -> Result<SourceRetirementPolicy, AudioPlatformError> {
        if request.backend() != AudioBackendKind::Wasapi
            || !matches!(request.mode(), AudioStreamMode::Shared(_))
            || max_frames == 0
            || max_frames > self.render_limit
        {
            return Err(AudioPlatformError::InvalidRequest);
        }
        let format = request.format();
        if format.sample_rate() != self.source.sample_rate()
            || format.encoding() != self.applied.encoding()
        {
            return Err(AudioPlatformError::InvalidFormat);
        }
        match requested_matrix {
            Some(matrix) => channel_remix::validate(self.source, format.pcm(), matrix)
                .map_err(|_| AudioPlatformError::InvalidFormat)?,
            None if format.pcm() != self.source => return Err(AudioPlatformError::InvalidFormat),
            None => {}
        }
        // Known geometry must fit before touching the live slot. DeviceDefault
        // still needs actual native-capacity validation at open.
        let check_frames = |frames: u128| {
            frames != 0 && frames <= max_frames as u128 && frames <= self.render_limit as u128
        };
        let duration_frames =
            |nanos: i64| (nanos as u128 * u128::from(format.sample_rate())).div_ceil(1_000_000_000);
        let buffer_fits = match request.buffer() {
            BufferRequest::DeviceDefault => true,
            BufferRequest::Frames(frames) => check_frames(u128::from(frames)),
            BufferRequest::Duration(duration) => check_frames(duration_frames(duration.as_nanos())),
        };
        let period_fits = match request.period() {
            PeriodRequest::DeviceDefault => true,
            PeriodRequest::Frames(frames) => check_frames(u128::from(frames)),
            PeriodRequest::Duration(duration) => check_frames(duration_frames(duration.as_nanos())),
        };
        if !buffer_fits || !period_fits {
            return Err(AudioPlatformError::Capacity);
        }
        Ok(
            if format == self.applied && requested_matrix == self.matrix.as_ref() {
                SourceRetirementPolicy::PreservePausedSuffix
            } else {
                SourceRetirementPolicy::RequireEmptySuffix
            },
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WasapiSourcePhase {
    Quarantined,
    Padding,
    Acquire,
    History,
    Render,
    Encode,
    Release,
    Admission,
    Clock,
    Start,
    Stop,
    Join,
    Recovery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WasapiSourceFailure {
    pub(crate) phase: WasapiSourcePhase,
    pub(crate) code: i32,
}

/// Numeric cold-report storage also carried through owner-thread cleanup/join.
/// Filling this report never allocates or replaces the original phase/code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WasapiSourceFailureReport {
    pub(crate) primary: Option<WasapiSourceFailure>,
    pub(crate) release: Option<i32>,
    pub(crate) stop: Option<i32>,
    pub(crate) start_cleanup: Option<i32>,
    pub(crate) join: Option<i32>,
    pub(crate) recovery: Option<i32>,
}

impl WasapiSourceFailureReport {
    pub(crate) fn record_primary(&mut self, phase: WasapiSourcePhase, code: i32) {
        self.primary
            .get_or_insert(WasapiSourceFailure { phase, code });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WasapiSourceStep {
    Continue,
    Retire,
    Failure(WasapiSourceFailureReport),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WasapiSourceWork {
    Fill { buffer_frames: u32 },
    ClockOnly,
}

/// WASAPI-specific owner-thread effects. Every acquired lease is consumed by
/// one explicit release, including local history/render/encode errors.
pub(crate) trait WasapiSourceOps {
    type Lease;

    fn available_frames(&mut self, buffer_frames: u32) -> Result<u32, i32>;
    fn acquire(&mut self, frames: u32) -> Result<Self::Lease, i32>;
    fn encode(
        &mut self,
        lease: &mut Self::Lease,
        format: DeviceFormat,
        samples: &[f32],
    ) -> Result<(), i32>;
    fn release(&mut self, lease: Self::Lease, silent: bool) -> Result<(), i32>;
    fn read_clock(&mut self) -> Result<(), i32>;

    fn history_request(&self) -> Option<SourceHistoryRetirementRequest> {
        None
    }

    fn render_pending(
        &mut self,
        state: &mut NativeOutputState,
        frames: usize,
    ) -> Result<RenderReport, i32> {
        state.render_pending(frames).map_err(|_| INVALID_ARGUMENT)
    }

    fn admitted(&mut self, _report: RenderReport, _frames: u32) {}
}

fn failure(phase: WasapiSourcePhase, code: i32) -> WasapiSourceStep {
    let mut report = WasapiSourceFailureReport::default();
    report.record_primary(phase, code);
    WasapiSourceStep::Failure(report)
}

/// Fill and timeout/control-wake clock observations enter this exact gate.
/// Do not decide again inside a synchronous lease: a concurrently submitted
/// retirement is evaluated against the complete resulting owner next time.
pub(crate) fn source_worker_step(
    state: &mut NativeOutputState,
    slot: &SourceRetirementSlot,
    ops: &mut impl WasapiSourceOps,
    work: WasapiSourceWork,
    completed_history: &mut u64,
) -> WasapiSourceStep {
    match slot.worker_decision(state) {
        WasapiSourceStep::Continue => {}
        decision => return decision,
    }
    if let WasapiSourceWork::Fill { buffer_frames } = work {
        let available = match ops.available_frames(buffer_frames) {
            Ok(frames) if frames <= buffer_frames => frames,
            Ok(_) => return failure(WasapiSourcePhase::Padding, INVALID_ARGUMENT),
            Err(code) => return failure(WasapiSourcePhase::Padding, code),
        };
        let frames = if state.pending_frames() == 0 {
            available
        } else {
            available.min(state.pending_frames().min(u32::MAX as usize) as u32)
        };
        if frames != 0 {
            let mut lease = match ops.acquire(frames) {
                Ok(lease) => lease,
                Err(code) => return failure(WasapiSourcePhase::Acquire, code),
            };
            let rendered = (|| {
                if let Some(request) = ops.history_request() {
                    state
                        .apply_source_history_retirement(request, completed_history)
                        .map_err(|_| (WasapiSourcePhase::History, INVALID_ARGUMENT))?;
                }
                let report = match state.pending_report() {
                    Some(report) => report,
                    None => ops
                        .render_pending(state, frames as usize)
                        .map_err(|code| (WasapiSourcePhase::Render, code))?,
                };
                let format = state
                    .format()
                    .ok_or((WasapiSourcePhase::Encode, INVALID_ARGUMENT))?;
                let count = (frames as usize)
                    .checked_mul(usize::from(format.channels()))
                    .ok_or((WasapiSourcePhase::Encode, INVALID_ARGUMENT))?;
                let samples = state
                    .pending_samples()
                    .get(..count)
                    .ok_or((WasapiSourcePhase::Encode, INVALID_ARGUMENT))?;
                ops.encode(&mut lease, format, samples)
                    .map_err(|code| (WasapiSourcePhase::Encode, code))?;
                Ok(report)
            })();
            let release = ops.release(lease, rendered.is_err());
            let mut errors = WasapiSourceFailureReport::default();
            if let Err((phase, code)) = rendered {
                errors.record_primary(phase, code);
            }
            if let Err(code) = release {
                state.mark_admission_uncertain();
                errors.record_primary(WasapiSourcePhase::Release, code);
                errors.release = Some(code);
            }
            if errors.primary.is_some() {
                return WasapiSourceStep::Failure(errors);
            }
            // Positive release owns admission even if the later clock fails.
            // Do not replay this prefix after recovery.
            if state.admit(frames as usize).is_err() {
                state.mark_admission_uncertain();
                return failure(WasapiSourcePhase::Admission, INVALID_ARGUMENT);
            }
            ops.admitted(
                rendered.expect("successful lease retains its report"),
                frames,
            );
        }
    }
    match ops.read_clock() {
        Ok(()) => WasapiSourceStep::Continue,
        Err(code) => failure(WasapiSourcePhase::Clock, code),
    }
}

#[cfg(test)]
#[path = "wasapi_source_worker_fixtures.rs"]
pub(crate) mod fixtures;
