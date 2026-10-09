//! Finite audio-output authority and original HOST input correspondence.
//! InputMerger owns acquired payloads; this owner reads no clock or device.
use crate::local_input::{InputMerger, MergeError};
use beatkernel::time::{
    presentation::ObservationAdmission, AffineClockMapper, CalibrationError, ClockDomainId,
    ClockInterval, ClockPair, ClockPoint, Duration, ExtrapolationPolicy, Timestamp,
};
use std::{collections::VecDeque, fmt};

/// Storage, freshness and explicit input-prediction permissions, not accuracy bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioAuthorityConfig {
    pub history_capacity: usize,
    pub max_observation_age: Duration,
    pub input_extrapolation: ExtrapolationPolicy,
    pub max_input_ahead: Duration,
}
impl Default for AudioAuthorityConfig {
    fn default() -> Self {
        Self {
            history_capacity: 64,
            max_observation_age: Duration::from_nanos(1_000_000_000),
            input_extrapolation: ExtrapolationPolicy::Forbid,
            max_input_ahead: Duration::ZERO,
        }
    }
}

/// Raw stream and logical audio origins remain distinct from acquisition HOST.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioAuthorityEpoch {
    pub id: u64,
    pub stream_origin: ClockPoint,
    pub logical_origin: ClockPoint,
    pub host_domain: ClockDomainId,
}
impl AudioAuthorityEpoch {
    /// Checked coordinate on this epoch's logical timeline.
    pub(crate) fn logical_output(self, raw: ClockPoint) -> Result<ClockPoint, AudioAuthorityError> {
        AudioAuthority::logical_in_epoch(self, raw)
    }
}

/// Rejection before changing retained observations or any watermark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioAuthorityError {
    InvalidConfig,
    DomainMismatch,
    WrongEpoch,
    ObservationRegression,
    PrefixRegression,
    OperationRegression,
    HistoryExpired,
    HistoryCapacity,
    PendingInputs,
    StalePreparation,
    Overflow,
    AllocationFailed,
    Calibration(CalibrationError),
    Merge(MergeError),
}
impl fmt::Display for AudioAuthorityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "audio authority: {self:?}")
    }
}
impl std::error::Error for AudioAuthorityError {}
impl From<CalibrationError> for AudioAuthorityError {
    fn from(error: CalibrationError) -> Self {
        Self::Calibration(error)
    }
}
impl From<MergeError> for AudioAuthorityError {
    fn from(error: MergeError) -> Self {
        Self::Merge(error)
    }
}

/// Exact semantic state relevant to preparing and committing one operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PreparationState {
    revision: u64,
    config: AudioAuthorityConfig,
    epoch: AudioAuthorityEpoch,
    first: Option<ClockPair>,
    latest: Option<ClockPair>,
    history_len: usize,
    acquired: Option<ClockPoint>,
    closed: Option<ClockPoint>,
    input_host: Option<ClockPoint>,
    operation: Option<ClockPoint>,
    presentation: Option<ClockPoint>,
}

/// Frozen mapping for one original acquired input. The caller retains its payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedInput {
    state: PreparationState,
    original: ClockPoint,
    now: ClockPoint,
    output: ClockPoint,
    mapper: AffineClockMapper,
}
impl PreparedInput {
    pub const fn original(&self) -> ClockPoint {
        self.original
    }
    pub const fn output(&self) -> ClockPoint {
        self.output
    }
    pub const fn mapper(&self) -> &AffineClockMapper {
        &self.mapper
    }
    pub const fn epoch(&self) -> u64 {
        self.state.epoch.id
    }
}

/// Actual observed presentation joined to the entire drained acquired HOST prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedFrontier {
    state: PreparationState,
    now: ClockPoint,
    host: ClockPoint,
    observed_host: ClockPoint,
    output: ClockPoint,
    advance: Option<ClockPoint>,
}
impl PreparedFrontier {
    pub const fn host(&self) -> ClockPoint {
        self.host
    }
    pub const fn observed_host(&self) -> ClockPoint {
        self.observed_host
    }
    pub const fn output(&self) -> ClockPoint {
        self.output
    }
    pub const fn advance(&self) -> Option<ClockPoint> {
        self.advance
    }
    pub const fn epoch(&self) -> u64 {
        self.state.epoch.id
    }
}

/// Native-authorized raw control boundary, separate from physical input history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedControlCutoff {
    state: PreparationState,
    epoch: u64,
    raw_output: ClockPoint,
    host_cutoff: ClockPoint,
    now: ClockPoint,
    output: ClockPoint,
    resume: bool,
}
impl PreparedControlCutoff {
    /// Whether pending input at the exact cutoff belongs to resumed playback.
    pub const fn is_resume(&self) -> bool {
        self.resume
    }
    /// Output creation epoch validated when preparing this native control boundary.
    pub const fn epoch(&self) -> u64 {
        self.epoch
    }
    pub const fn host(&self) -> ClockPoint {
        self.host_cutoff
    }
    pub const fn output(&self) -> ClockPoint {
        self.output
    }
    pub const fn raw_output(&self) -> ClockPoint {
        self.raw_output
    }
}

/// Actual presentation and acquired-prefix closure while the producer is held.
/// This descriptor grants no Runtime operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedHeldFrontier {
    frontier: PreparedFrontier,
}
impl PreparedHeldFrontier {
    pub const fn host(&self) -> ClockPoint {
        self.frontier.host
    }
    pub const fn output(&self) -> ClockPoint {
        self.frontier.output
    }
    pub const fn observed_host(&self) -> ClockPoint {
        self.frontier.observed_host
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedEpoch {
    state: PreparationState,
    next: AudioAuthorityEpoch,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedRestart {
    state: PreparationState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrimedCorrelationStage {
    Epoch(PreparedEpoch),
    Restart(PreparedRestart),
}

/// Two original observations staged for atomic correlation publication.
/// Gameplay watermarks remain with the active owner until and after commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedPrimedCorrelation {
    stage: PrimedCorrelationStage,
    pairs: [ClockPair; 2],
    now: ClockPoint,
}

/// Cold-preallocated real observations; no duplicate acquired-input queue or rate loop.
#[derive(Debug)]
pub struct AudioAuthority {
    config: AudioAuthorityConfig,
    epoch: AudioAuthorityEpoch,
    history: VecDeque<ClockPair>,
    revision: u64,
    acquired: Option<ClockPoint>,
    closed: Option<ClockPoint>,
    input_host: Option<ClockPoint>,
    operation: Option<ClockPoint>,
    presentation: Option<ClockPoint>,
}
impl AudioAuthority {
    pub fn new(
        config: AudioAuthorityConfig,
        epoch: AudioAuthorityEpoch,
    ) -> Result<Self, AudioAuthorityError> {
        let negative_extrapolation = matches!(config.input_extrapolation,
            ExtrapolationPolicy::Bounded { before, after } if before < Duration::ZERO || after < Duration::ZERO);
        if !(2..=1024).contains(&config.history_capacity)
            || config.max_observation_age <= Duration::ZERO
            || config.max_input_ahead < Duration::ZERO
            || negative_extrapolation
        {
            return Err(AudioAuthorityError::InvalidConfig);
        }
        validate_epoch_domains(epoch)?;
        let mut history = VecDeque::new();
        history
            .try_reserve_exact(config.history_capacity)
            .map_err(|_| AudioAuthorityError::AllocationFailed)?;
        Ok(Self {
            config,
            epoch,
            history,
            revision: 0,
            acquired: None,
            closed: None,
            input_host: None,
            operation: None,
            presentation: None,
        })
    }
    pub const fn config(&self) -> AudioAuthorityConfig {
        self.config
    }
    pub const fn epoch(&self) -> AudioAuthorityEpoch {
        self.epoch
    }
    pub fn latest_observation(&self) -> Option<ClockPair> {
        self.history.back().copied()
    }
    pub const fn acquired_prefix(&self) -> Option<ClockPoint> {
        self.acquired
    }
    pub const fn closed_host_prefix(&self) -> Option<ClockPoint> {
        self.closed
    }
    pub const fn committed_input_host(&self) -> Option<ClockPoint> {
        self.input_host
    }
    pub const fn committed_operation(&self) -> Option<ClockPoint> {
        self.operation
    }
    pub const fn committed_presentation(&self) -> Option<ClockPoint> {
        self.presentation
    }
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// Retain only progressing original evidence. Unchanged output never refreshes age.
    pub fn observe(
        &mut self,
        epoch: u64,
        pair: ClockPair,
    ) -> Result<ObservationAdmission, AudioAuthorityError> {
        if epoch != self.epoch.id {
            return Err(AudioAuthorityError::WrongEpoch);
        }
        if pair.source.domain != self.epoch.stream_origin.domain
            || pair.target.domain != self.epoch.host_domain
        {
            return Err(AudioAuthorityError::DomainMismatch);
        }
        let output = self.logical(pair.source)?;
        if self
            .presentation
            .is_some_and(|last| output.timestamp < last.timestamp)
        {
            return Err(AudioAuthorityError::ObservationRegression);
        }
        if let Some(previous) = self.latest_observation() {
            if pair.source.timestamp < previous.source.timestamp
                || pair.target.timestamp < previous.target.timestamp
            {
                return Err(AudioAuthorityError::ObservationRegression);
            }
            if pair.source.timestamp == previous.source.timestamp {
                return Ok(ObservationAdmission::Unchanged);
            }
            if pair.target.timestamp == previous.target.timestamp {
                return Err(AudioAuthorityError::ObservationRegression);
            }
            // Validate the complete new observed interval before changing storage.
            self.mapper(
                previous,
                pair,
                ClockInterval {
                    start: previous.target.timestamp,
                    end: pair.target.timestamp,
                },
                ExtrapolationPolicy::Forbid,
            )?;
        }
        let retire = self.history.len() == self.config.history_capacity;
        if retire
            && !self
                .closed
                .is_some_and(|closed| self.history[1].target.timestamp <= closed.timestamp)
        {
            return Err(AudioAuthorityError::HistoryCapacity);
        }
        let revision = self.next_revision()?;
        if retire {
            self.history.pop_front();
        }
        self.history.push_back(pair);
        self.revision = revision;
        Ok(ObservationAdmission::Retained)
    }

    /// Acquisition coverage alone neither consumes input nor grants audio progress.
    pub fn record_acquired_prefix(
        &mut self,
        prefix: ClockPoint,
    ) -> Result<(), AudioAuthorityError> {
        self.validate_host(prefix)?;
        if self
            .acquired
            .is_some_and(|last| prefix.timestamp < last.timestamp)
        {
            return Err(AudioAuthorityError::PrefixRegression);
        }
        if self.acquired == Some(prefix) {
            return Ok(());
        }
        let revision = self.next_revision()?;
        self.acquired = Some(prefix);
        self.revision = revision;
        Ok(())
    }

    /// Prepare before popping the earliest merger input; retain unknown-quality mapping.
    pub fn prepare_input(
        &self,
        original: ClockPoint,
        now: ClockPoint,
    ) -> Result<Option<PreparedInput>, AudioAuthorityError> {
        self.validate_host(original)?;
        self.validate_host(now)?;
        if self
            .closed
            .is_some_and(|closed| original.timestamp <= closed.timestamp)
            || self
                .input_host
                .is_some_and(|last| original.timestamp < last.timestamp)
        {
            return Err(AudioAuthorityError::OperationRegression);
        }
        if original.timestamp > now.timestamp
            || self
                .acquired
                .is_some_and(|prefix| prefix.timestamp > now.timestamp)
            || self
                .acquired
                .is_none_or(|prefix| original.timestamp > prefix.timestamp)
            || self.history.len() < 2
            || !self.fresh(now)?
        {
            return Ok(None);
        }
        let first = self.history[0];
        let latest = *self.history.back().expect("two real anchors were checked");
        let (left, right, validity, extrapolation) = if original.timestamp < first.target.timestamp
        {
            let ExtrapolationPolicy::Bounded { before, .. } = self.config.input_extrapolation
            else {
                return Err(AudioAuthorityError::HistoryExpired);
            };
            let start = add(first.target.timestamp, -i128::from(before.as_nanos()))?;
            if original.timestamp < start {
                return Err(AudioAuthorityError::HistoryExpired);
            }
            (
                first,
                self.history[1],
                ClockInterval {
                    start,
                    end: self.history[1].target.timestamp,
                },
                ExtrapolationPolicy::Bounded {
                    before,
                    after: Duration::ZERO,
                },
            )
        } else if original.timestamp > latest.target.timestamp {
            let ExtrapolationPolicy::Bounded { after, .. } = self.config.input_extrapolation else {
                return Ok(None);
            };
            let end = add(latest.target.timestamp, i128::from(after.as_nanos()))?;
            if original.timestamp > end {
                return Ok(None);
            }
            let left = self.history[self.history.len() - 2];
            (
                left,
                latest,
                ClockInterval {
                    start: left.target.timestamp,
                    end,
                },
                ExtrapolationPolicy::Bounded {
                    before: Duration::ZERO,
                    after,
                },
            )
        } else {
            let index = (0..self.history.len() - 1)
                .find(|&index| original.timestamp <= self.history[index + 1].target.timestamp)
                .expect("point lies within retained real anchors");
            let left = self.history[index];
            let right = self.history[index + 1];
            (
                left,
                right,
                ClockInterval {
                    start: left.target.timestamp,
                    end: right.target.timestamp,
                },
                ExtrapolationPolicy::Forbid,
            )
        };
        let mapper = self.mapper(left, right, validity, extrapolation)?;
        let output = ClockPoint {
            domain: self.epoch.logical_origin.domain,
            timestamp: mapper.map_checked(original, self.epoch.logical_origin.domain)?,
        };
        if original.timestamp > latest.target.timestamp
            && delta(output.timestamp, self.logical(latest.source)?.timestamp)
                > i128::from(self.config.max_input_ahead.as_nanos())
        {
            return Ok(None);
        }
        if self
            .operation
            .is_some_and(|last| output.timestamp < last.timestamp)
        {
            return Err(AudioAuthorityError::OperationRegression);
        }
        self.next_revision()?;
        Ok(Some(PreparedInput {
            state: self.state(),
            original,
            now,
            output,
            mapper,
        }))
    }

    /// Commit the actual Runtime operation once; post-report observer faults do not undo it.
    pub fn commit_input(&mut self, prepared: PreparedInput) -> Result<(), AudioAuthorityError> {
        if prepared.state != self.state()
            || self.prepare_input(prepared.original, prepared.now)? != Some(prepared)
        {
            return Err(AudioAuthorityError::StalePreparation);
        }
        let revision = self.next_revision()?;
        self.input_host = Some(prepared.original);
        self.operation = Some(prepared.output);
        self.revision = revision;
        Ok(())
    }

    /// Only a real observation covered by a fully drained HOST prefix grants presentation.
    pub fn prepare_frontier(
        &self,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<Option<PreparedFrontier>, AudioAuthorityError> {
        self.validate_host(now)?;
        let Some(host) = self.acquired else {
            return Ok(None);
        };
        if merger.peek_ready(host)?.is_some()
            || host.timestamp > now.timestamp
            || self.history.len() < 2
            || !self.fresh(now)?
        {
            return Ok(None);
        }
        let Some(pair) = self
            .history
            .iter()
            .rev()
            .find(|pair| pair.target.timestamp <= host.timestamp)
        else {
            return Ok(None);
        };
        if !self.pair_is_fresh(*pair, now) {
            return Ok(None);
        }
        let output = self.logical(pair.source)?;
        if self
            .presentation
            .is_some_and(|last| output.timestamp < last.timestamp)
        {
            return Err(AudioAuthorityError::ObservationRegression);
        }
        if self.closed == Some(host) && self.presentation == Some(output) {
            return Ok(None);
        }
        let advance = self
            .operation
            .is_none_or(|last| output.timestamp > last.timestamp)
            .then_some(output);
        self.next_revision()?;
        Ok(Some(PreparedFrontier {
            state: self.state(),
            now,
            host,
            observed_host: pair.target,
            output,
            advance,
        }))
    }

    pub fn commit_frontier(
        &mut self,
        prepared: PreparedFrontier,
        merger: &mut InputMerger,
    ) -> Result<(), AudioAuthorityError> {
        if prepared.state != self.state()
            || self.prepare_frontier(prepared.now, merger)? != Some(prepared)
        {
            return Err(AudioAuthorityError::StalePreparation);
        }
        let revision = self.next_revision()?;
        // Existing commit rechecks the entire prefix before either owner changes.
        merger.commit(prepared.host)?;
        self.closed = Some(prepared.host);
        self.presentation = Some(prepared.output);
        if let Some(output) = prepared.advance {
            self.operation = Some(output);
        }
        self.revision = revision;
        Ok(())
    }

    /// Prepares an operation from a validated native physical pause boundary.
    /// The caller supplies native authorization; HOST does not infer raw output.
    /// Missing evidence or undrained pre-cutoff input holds without consumption.
    pub fn prepare_control_cutoff(
        &self,
        epoch: u64,
        raw_output: ClockPoint,
        host_cutoff: ClockPoint,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<Option<PreparedControlCutoff>, AudioAuthorityError> {
        self.prepare_control_inner(epoch, raw_output, host_cutoff, now, merger, false)
    }

    /// Locate a raw boundary strictly inside retained real native associations.
    /// No extrapolation or acquisition/receive-time substitution is permitted.
    /// The HOST cut is rounded upward: integer HOST points below it precede
    /// the raw boundary, and points at or above it belong to the new attempt.
    pub fn presented_boundary_host(
        &self,
        raw: ClockPoint,
        now: ClockPoint,
    ) -> Result<Option<ClockPoint>, AudioAuthorityError> {
        self.validate_host(now)?;
        self.logical(raw)?;
        if raw.timestamp < self.epoch.stream_origin.timestamp {
            return Err(AudioAuthorityError::ObservationRegression);
        }
        let Some(first) = self.history.front() else {
            return Ok(None);
        };
        if raw.timestamp < first.source.timestamp {
            return Err(AudioAuthorityError::HistoryExpired);
        }
        if self.history.len() < 2 || !self.fresh(now)? {
            return Ok(None);
        }
        let Some(index) = (0..self.history.len() - 1)
            .find(|&i| raw.timestamp <= self.history[i + 1].source.timestamp)
        else {
            return Ok(None);
        };
        let left = self.history[index];
        let right = self.history[index + 1];
        // Associations captured after `now` cannot authorize this operation.
        if right.target.timestamp > now.timestamp {
            return Ok(None);
        }
        let raw_span = delta(right.source.timestamp, left.source.timestamp);
        let host_span = delta(right.target.timestamp, left.target.timestamp);
        let distance = delta(raw.timestamp, left.source.timestamp);
        if raw_span <= 0 || host_span <= 0 || distance < 0 {
            return Err(AudioAuthorityError::ObservationRegression);
        }
        // Every timestamp difference fits u64; its full product fits u128
        // even when it would overflow signed i128 on a long output epoch.
        let product = (distance as u128)
            .checked_mul(host_span as u128)
            .ok_or(AudioAuthorityError::Overflow)?;
        let span = raw_span as u128;
        let offset = product / span + u128::from(product % span != 0);
        Ok(Some(ClockPoint {
            domain: self.epoch.host_domain,
            timestamp: add(
                left.target.timestamp,
                i128::try_from(offset).map_err(|_| AudioAuthorityError::Overflow)?,
            )?,
        }))
    }

    /// Prepare resume without consuming original input at the exact HOST cutoff.
    pub fn prepare_resume_control_cutoff(
        &self,
        epoch: u64,
        raw_output: ClockPoint,
        host_cutoff: ClockPoint,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<Option<PreparedControlCutoff>, AudioAuthorityError> {
        self.prepare_control_inner(epoch, raw_output, host_cutoff, now, merger, true)
    }

    fn prepare_control_inner(
        &self,
        epoch: u64,
        raw_output: ClockPoint,
        host_cutoff: ClockPoint,
        now: ClockPoint,
        merger: &InputMerger,
        resume: bool,
    ) -> Result<Option<PreparedControlCutoff>, AudioAuthorityError> {
        if epoch != self.epoch.id {
            return Err(AudioAuthorityError::WrongEpoch);
        }
        self.validate_host(host_cutoff)?;
        self.validate_host(now)?;
        let output = self.logical(raw_output)?;
        if raw_output.timestamp < self.epoch.stream_origin.timestamp {
            return Err(AudioAuthorityError::ObservationRegression);
        }
        if self
            .operation
            .is_some_and(|last| output.timestamp < last.timestamp)
            || self
                .input_host
                .is_some_and(|last| host_cutoff.timestamp < last.timestamp)
        {
            return Err(AudioAuthorityError::OperationRegression);
        }
        let Some(prefix) = self.acquired else {
            return Ok(None);
        };
        if host_cutoff.timestamp > now.timestamp
            || host_cutoff.timestamp > prefix.timestamp
            || self.history.len() < 2
            || !self.fresh(now)?
            || raw_output.timestamp
                > self
                    .latest_observation()
                    .expect("two anchors were checked")
                    .source
                    .timestamp
        {
            return Ok(None);
        }
        // The cutoff may precede an already closed prefix. Inspect at the full
        // acquired prefix, then compare the earliest exact event to the cutoff.
        if merger.peek_ready(prefix)?.is_some_and(|event| {
            event.meta().timestamp < host_cutoff.timestamp
                || (!resume && event.meta().timestamp == host_cutoff.timestamp)
        }) {
            return Ok(None);
        }
        self.next_revision()?;
        Ok(Some(PreparedControlCutoff {
            state: self.state(),
            epoch,
            raw_output,
            host_cutoff,
            now,
            output,
            resume,
        }))
    }

    /// Commits only a genuine accepted Runtime control operation, before observers.
    /// No input occurrence, presentation, prefix or correlation history is invented.
    pub fn commit_control_cutoff(
        &mut self,
        prepared: PreparedControlCutoff,
        merger: &InputMerger,
    ) -> Result<(), AudioAuthorityError> {
        if prepared.state != self.state()
            || self.prepare_control_inner(
                prepared.epoch,
                prepared.raw_output,
                prepared.host_cutoff,
                prepared.now,
                merger,
                prepared.resume,
            )? != Some(prepared)
        {
            return Err(AudioAuthorityError::StalePreparation);
        }
        let revision = self.next_revision()?;
        self.operation = Some(prepared.output);
        self.revision = revision;
        Ok(())
    }

    /// Prepares actual presentation/prefix closure without granting Runtime advance.
    /// The caller must hold the acknowledged producer throughout this operation.
    pub fn prepare_held_frontier(
        &self,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<Option<PreparedHeldFrontier>, AudioAuthorityError> {
        Ok(self
            .prepare_frontier(now, merger)?
            .map(|frontier| PreparedHeldFrontier { frontier }))
    }

    /// Closes only the acquired HOST prefix and actual presentation while held.
    pub fn commit_held_frontier(
        &mut self,
        prepared: PreparedHeldFrontier,
        merger: &mut InputMerger,
    ) -> Result<(), AudioAuthorityError> {
        if prepared.frontier.state != self.state()
            || self.prepare_held_frontier(prepared.frontier.now, merger)? != Some(prepared)
        {
            return Err(AudioAuthorityError::StalePreparation);
        }
        let revision = self.next_revision()?;
        merger.commit(prepared.frontier.host)?;
        self.closed = Some(prepared.frontier.host);
        self.presentation = Some(prepared.frontier.output);
        self.revision = revision;
        Ok(())
    }

    pub fn prepare_epoch(
        &self,
        next: AudioAuthorityEpoch,
        merger: &InputMerger,
    ) -> Result<PreparedEpoch, AudioAuthorityError> {
        validate_epoch_domains(next)?;
        if next.id <= self.epoch.id {
            return Err(AudioAuthorityError::WrongEpoch);
        }
        if next.logical_origin.domain != self.epoch.logical_origin.domain
            || next.host_domain != self.epoch.host_domain
        {
            return Err(AudioAuthorityError::DomainMismatch);
        }
        if self
            .operation
            .is_some_and(|last| next.logical_origin.timestamp < last.timestamp)
        {
            return Err(AudioAuthorityError::OperationRegression);
        }
        if merger.pending() != 0 {
            return Err(AudioAuthorityError::PendingInputs);
        }
        self.next_revision()?;
        Ok(PreparedEpoch {
            state: self.state(),
            next,
        })
    }
    pub fn commit_epoch(
        &mut self,
        prepared: PreparedEpoch,
        merger: &InputMerger,
    ) -> Result<(), AudioAuthorityError> {
        if prepared.state != self.state() || self.prepare_epoch(prepared.next, merger)? != prepared
        {
            return Err(AudioAuthorityError::StalePreparation);
        }
        let revision = self.next_revision()?;
        self.epoch = prepared.next;
        self.history.clear();
        self.revision = revision;
        Ok(())
    }
    pub fn prepare_correlation_restart(
        &self,
        merger: &InputMerger,
    ) -> Result<PreparedRestart, AudioAuthorityError> {
        if merger.pending() != 0 {
            return Err(AudioAuthorityError::PendingInputs);
        }
        self.next_revision()?;
        Ok(PreparedRestart {
            state: self.state(),
        })
    }
    pub fn commit_correlation_restart(
        &mut self,
        prepared: PreparedRestart,
        merger: &InputMerger,
    ) -> Result<(), AudioAuthorityError> {
        if prepared.state != self.state() || self.prepare_correlation_restart(merger)? != prepared {
            return Err(AudioAuthorityError::StalePreparation);
        }
        let revision = self.next_revision()?;
        self.history.clear();
        self.revision = revision;
        Ok(())
    }

    /// Stages two real anchors for a prepared replacement without changing history.
    pub fn prepare_primed_epoch(
        &self,
        staged: PreparedEpoch,
        pairs: [ClockPair; 2],
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<PreparedPrimedCorrelation, AudioAuthorityError> {
        self.prepare_primed_correlation(PrimedCorrelationStage::Epoch(staged), pairs, now, merger)
    }

    /// Stages a same-epoch restart while retaining every accepted gameplay watermark.
    pub fn prepare_primed_restart(
        &self,
        staged: PreparedRestart,
        pairs: [ClockPair; 2],
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<PreparedPrimedCorrelation, AudioAuthorityError> {
        self.prepare_primed_correlation(PrimedCorrelationStage::Restart(staged), pairs, now, merger)
    }

    fn prepare_primed_correlation(
        &self,
        stage: PrimedCorrelationStage,
        pairs: [ClockPair; 2],
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<PreparedPrimedCorrelation, AudioAuthorityError> {
        let prepared = PreparedPrimedCorrelation { stage, pairs, now };
        self.validate_primed_correlation(&prepared, now, merger)?;
        Ok(prepared)
    }

    /// Rechecks staged identity, pending input, chronological bounds and freshness.
    /// Publication may be later than staging, but may not rewind its HOST sample.
    pub fn validate_primed_correlation(
        &self,
        prepared: &PreparedPrimedCorrelation,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<(), AudioAuthorityError> {
        let epoch = match prepared.stage {
            PrimedCorrelationStage::Epoch(staged) => {
                if staged.state != self.state()
                    || self.prepare_epoch(staged.next, merger)? != staged
                {
                    return Err(AudioAuthorityError::StalePreparation);
                }
                staged.next
            }
            PrimedCorrelationStage::Restart(staged) => {
                if staged.state != self.state()
                    || self.prepare_correlation_restart(merger)? != staged
                {
                    return Err(AudioAuthorityError::StalePreparation);
                }
                self.epoch
            }
        };
        self.validate_host(prepared.now)?;
        self.validate_host(now)?;
        if now.timestamp < prepared.now.timestamp {
            return Err(AudioAuthorityError::ObservationRegression);
        }
        let [first, second] = prepared.pairs;
        for pair in prepared.pairs {
            if pair.source.domain != epoch.stream_origin.domain
                || pair.target.domain != epoch.host_domain
            {
                return Err(AudioAuthorityError::DomainMismatch);
            }
            if pair.target.timestamp > now.timestamp {
                return Err(AudioAuthorityError::ObservationRegression);
            }
            let output = Self::logical_in_epoch(epoch, pair.source)?;
            if self
                .presentation
                .is_some_and(|last| output.timestamp < last.timestamp)
            {
                return Err(AudioAuthorityError::ObservationRegression);
            }
        }
        if first.source.timestamp >= second.source.timestamp
            || first.target.timestamp >= second.target.timestamp
        {
            return Err(AudioAuthorityError::ObservationRegression);
        }
        for last in [
            self.input_host,
            self.closed,
            self.latest_observation().map(|pair| pair.target),
        ]
        .into_iter()
        .flatten()
        {
            if second.target.timestamp < last.timestamp {
                return Err(AudioAuthorityError::ObservationRegression);
            }
        }
        if !self.pair_is_fresh(second, now) {
            return Err(AudioAuthorityError::HistoryExpired);
        }
        Self::mapper_in_epoch(
            epoch,
            first,
            second,
            ClockInterval {
                start: first.target.timestamp,
                end: second.target.timestamp,
            },
            ExtrapolationPolicy::Forbid,
        )?;
        Ok(())
    }

    /// Publishes the staged two-anchor correlation using reserved history storage.
    /// This never advances the judged operation or committed presentation/prefix.
    pub fn commit_primed_correlation(
        &mut self,
        prepared: PreparedPrimedCorrelation,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<(), AudioAuthorityError> {
        self.validate_primed_correlation(&prepared, now, merger)?;
        let revision = self.next_revision()?;
        if let PrimedCorrelationStage::Epoch(staged) = prepared.stage {
            self.epoch = staged.next;
        }
        self.history.clear();
        self.history.extend(prepared.pairs);
        self.revision = revision;
        Ok(())
    }
    fn state(&self) -> PreparationState {
        PreparationState {
            revision: self.revision,
            config: self.config,
            epoch: self.epoch,
            first: self.history.front().copied(),
            latest: self.latest_observation(),
            history_len: self.history.len(),
            acquired: self.acquired,
            closed: self.closed,
            input_host: self.input_host,
            operation: self.operation,
            presentation: self.presentation,
        }
    }
    fn next_revision(&self) -> Result<u64, AudioAuthorityError> {
        self.revision
            .checked_add(1)
            .ok_or(AudioAuthorityError::Overflow)
    }
    fn validate_host(&self, point: ClockPoint) -> Result<(), AudioAuthorityError> {
        if point.domain != self.epoch.host_domain {
            Err(AudioAuthorityError::DomainMismatch)
        } else {
            Ok(())
        }
    }
    fn logical(&self, stream: ClockPoint) -> Result<ClockPoint, AudioAuthorityError> {
        Self::logical_in_epoch(self.epoch, stream)
    }
    pub(crate) fn checked_logical_output(
        &self,
        raw: ClockPoint,
    ) -> Result<ClockPoint, AudioAuthorityError> {
        self.logical(raw)
    }
    fn logical_in_epoch(
        epoch: AudioAuthorityEpoch,
        stream: ClockPoint,
    ) -> Result<ClockPoint, AudioAuthorityError> {
        if stream.domain != epoch.stream_origin.domain {
            return Err(AudioAuthorityError::DomainMismatch);
        }
        Ok(ClockPoint {
            domain: epoch.logical_origin.domain,
            timestamp: add(
                epoch.logical_origin.timestamp,
                delta(stream.timestamp, epoch.stream_origin.timestamp),
            )?,
        })
    }
    fn mapper(
        &self,
        first: ClockPair,
        second: ClockPair,
        validity: ClockInterval,
        extrapolation: ExtrapolationPolicy,
    ) -> Result<AffineClockMapper, AudioAuthorityError> {
        Self::mapper_in_epoch(self.epoch, first, second, validity, extrapolation)
    }
    fn mapper_in_epoch(
        epoch: AudioAuthorityEpoch,
        first: ClockPair,
        second: ClockPair,
        validity: ClockInterval,
        extrapolation: ExtrapolationPolicy,
    ) -> Result<AffineClockMapper, AudioAuthorityError> {
        Ok(AffineClockMapper::from_pairs_unknown(
            ClockPair {
                source: first.target,
                target: Self::logical_in_epoch(epoch, first.source)?,
            },
            ClockPair {
                source: second.target,
                target: Self::logical_in_epoch(epoch, second.source)?,
            },
            validity,
            extrapolation,
        )?)
    }
    fn fresh(&self, now: ClockPoint) -> Result<bool, AudioAuthorityError> {
        self.validate_host(now)?;
        Ok(self
            .history
            .iter()
            .rev()
            .any(|pair| self.pair_is_fresh(*pair, now)))
    }
    /// Read-only availability from retained, already due native associations.
    pub(crate) fn has_fresh_observation(
        &self,
        now: ClockPoint,
    ) -> Result<bool, AudioAuthorityError> {
        self.fresh(now)
    }
    fn pair_is_fresh(&self, pair: ClockPair, now: ClockPoint) -> bool {
        let age = delta(now.timestamp, pair.target.timestamp);
        age >= 0 && age <= i128::from(self.config.max_observation_age.as_nanos())
    }
}
fn validate_epoch_domains(epoch: AudioAuthorityEpoch) -> Result<(), AudioAuthorityError> {
    if epoch.host_domain == epoch.stream_origin.domain
        || epoch.host_domain == epoch.logical_origin.domain
        || epoch.stream_origin.domain == epoch.logical_origin.domain
    {
        Err(AudioAuthorityError::DomainMismatch)
    } else {
        Ok(())
    }
}
fn delta(later: Timestamp, earlier: Timestamp) -> i128 {
    i128::from(later.as_nanos()) - i128::from(earlier.as_nanos())
}
fn add(origin: Timestamp, offset: i128) -> Result<Timestamp, AudioAuthorityError> {
    i64::try_from(i128::from(origin.as_nanos()) + offset)
        .map(Timestamp::from_nanos)
        .map_err(|_| AudioAuthorityError::Overflow)
}

#[cfg(test)]
#[path = "audio_authority_fixtures.rs"]
mod audio_authority_fixtures;

#[cfg(test)]
#[path = "audio_authority_priming_fixtures.rs"]
mod priming_fixtures;

#[cfg(test)]
#[path = "audio_authority_control_fixtures.rs"]
mod control_fixtures;

#[cfg(test)]
mod practice_cut_tests {
    use super::*;
    fn point(domain: u32, ns: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(domain),
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn owner() -> AudioAuthority {
        AudioAuthority::new(
            AudioAuthorityConfig::default(),
            AudioAuthorityEpoch {
                id: 8,
                stream_origin: point(2, 100),
                logical_origin: point(3, 500),
                host_domain: ClockDomainId(1),
            },
        )
        .unwrap()
    }
    #[test]
    fn inverse_native_cut_uses_ceil_and_never_control_receive_time() {
        let mut authority = owner();
        authority
            .observe(
                8,
                ClockPair {
                    source: point(2, 100),
                    target: point(1, 1000),
                },
            )
            .unwrap();
        authority
            .observe(
                8,
                ClockPair {
                    source: point(2, 103),
                    target: point(1, 1002),
                },
            )
            .unwrap();
        assert_eq!(
            authority
                .presented_boundary_host(point(2, 101), point(1, 1002))
                .unwrap(),
            Some(point(1, 1001))
        );
        assert_eq!(
            authority
                .presented_boundary_host(point(2, 102), point(1, 1002))
                .unwrap(),
            Some(point(1, 1002))
        );
        assert_eq!(
            authority
                .presented_boundary_host(point(2, 104), point(1, 1002))
                .unwrap(),
            None
        );
        assert_eq!(
            authority
                .presented_boundary_host(point(2, 101), point(1, 1001))
                .unwrap(),
            None
        );
        assert!(matches!(
            authority.presented_boundary_host(point(7, 101), point(1, 1002)),
            Err(AudioAuthorityError::DomainMismatch)
        ));
        assert_eq!(authority.committed_operation(), None);
        assert_eq!(authority.acquired_prefix(), None);
    }
    #[test]
    fn full_timestamp_span_inverse_avoids_signed_product_overflow() {
        let mut authority = AudioAuthority::new(
            AudioAuthorityConfig::default(),
            AudioAuthorityEpoch {
                id: 9,
                stream_origin: point(2, i64::MIN),
                logical_origin: point(3, i64::MIN),
                host_domain: ClockDomainId(1),
            },
        )
        .unwrap();
        authority
            .observe(
                9,
                ClockPair {
                    source: point(2, i64::MIN),
                    target: point(1, i64::MIN),
                },
            )
            .unwrap();
        authority
            .observe(
                9,
                ClockPair {
                    source: point(2, i64::MAX),
                    target: point(1, i64::MAX),
                },
            )
            .unwrap();
        assert_eq!(
            authority
                .presented_boundary_host(point(2, i64::MAX - 1), point(1, i64::MAX))
                .unwrap(),
            Some(point(1, i64::MAX - 1))
        );
    }
}
