//! Finite audio-output authority and original HOST input correspondence.
//! InputMerger owns acquired payloads; this owner reads no clock or device.
use crate::local_input::{InputMerger, MergeError};
use crate::native_output_coordinate::NativeOutputCoordinate;
use beatkernel::audio::{InterpolatedTargetTime, OutputFrameBasis, TargetFrameBasis, TargetTime};
use beatkernel::time::{
    presentation::ObservationAdmission, AffineClockMapper, CalibrationError, ClockDomainId,
    ClockInterval, ClockMapper, ClockMappingQuality, ClockPair, ClockPoint, Duration,
    ExtrapolationPolicy, Timestamp,
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
    /// Checked mathematical projection onto this epoch's logical timeline.
    ///
    /// This conversion does not acquire native evidence, prove submission, admit
    /// an observation or acknowledge playback control, and mutates no authority.
    /// Callers must separately qualify the original native coordinate and HOST
    /// before using this projection for playback.
    pub fn logical_output(self, raw: ClockPoint) -> Result<ClockPoint, AudioAuthorityError> {
        AudioAuthority::logical_in_epoch(self, raw)
    }
}

/// Immutable original-coordinate descriptor. This validates mathematical consistency
/// only; it grants no native evidence, submission, epoch admission or pause ACK.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioAuthorityCoordinateBasis {
    SourceFrame(OutputFrameBasis),
    SourceCounter {
        basis: OutputFrameBasis,
        frequency: u64,
    },
    TargetFrame(TargetFrameBasis),
}
impl AudioAuthorityCoordinateBasis {
    fn descriptor(self) -> Result<(TargetTime, u64, ClockPoint), AudioAuthorityError> {
        match self {
            Self::SourceFrame(basis) => Ok((
                TargetTime::from_frames(basis.start_physical_frame(), basis.sample_rate())
                    .map_err(|_| AudioAuthorityError::Overflow)?,
                u64::from(basis.sample_rate()),
                basis.origin(),
            )),
            Self::SourceCounter { basis, frequency } => Ok((
                TargetTime::from_frames(basis.start_physical_frame(), basis.sample_rate())
                    .map_err(|_| AudioAuthorityError::Overflow)?,
                frequency,
                basis.origin(),
            )),
            Self::TargetFrame(basis) => Ok((
                basis.start_time(),
                u64::from(basis.sample_rate()),
                basis.origin(),
            )),
        }
    }
    fn query(
        self,
        observation: AudioAuthorityObservation,
    ) -> Result<InterpolatedTargetTime, AudioAuthorityError> {
        let (base, frequency, _) = self.descriptor()?;
        let units = CoordinateInputMapper::units(observation)?;
        InterpolatedTargetTime::new(base, units, units, frequency, 0, 1)
            .map_err(|_| AudioAuthorityError::Overflow)
    }
    pub(crate) fn interpolate(
        self,
        left: AudioAuthorityObservation,
        right: AudioAuthorityObservation,
        host: ClockPoint,
    ) -> Result<InterpolatedTargetTime, AudioAuthorityError> {
        if host.domain != left.pair.target.domain || host.domain != right.pair.target.domain {
            return Err(AudioAuthorityError::DomainMismatch);
        }
        let left_units = CoordinateInputMapper::units(left)?;
        let right_units = CoordinateInputMapper::units(right)?;
        if right_units <= left_units || right.pair.target.timestamp <= left.pair.target.timestamp {
            return Err(AudioAuthorityError::ObservationRegression);
        }
        let (base, frequency, _) = self.descriptor()?;
        let host_span = u64::try_from(delta(
            right.pair.target.timestamp,
            left.pair.target.timestamp,
        ))
        .map_err(|_| AudioAuthorityError::Overflow)?;
        InterpolatedTargetTime::new(
            base,
            left_units,
            right_units,
            frequency,
            delta(host.timestamp, left.pair.target.timestamp),
            host_span,
        )
        .map_err(|_| AudioAuthorityError::Overflow)
    }
    fn point(self, coordinate: NativeOutputCoordinate) -> Result<ClockPoint, AudioAuthorityError> {
        match (self, coordinate) {
            (Self::SourceFrame(basis), NativeOutputCoordinate::StreamFrame { frame }) => basis
                .point_at_stream_frame(frame)
                .map_err(|_| AudioAuthorityError::Overflow),
            (Self::TargetFrame(basis), NativeOutputCoordinate::StreamFrame { frame }) => basis
                .point_at_stream_frame(frame)
                .map_err(|_| AudioAuthorityError::Overflow),
            (
                Self::SourceCounter { basis, frequency },
                NativeOutputCoordinate::Counter {
                    position,
                    frequency: actual,
                },
            ) if frequency > 0 && actual == frequency => basis
                .point_at_native_counter(position, frequency)
                .map_err(|_| AudioAuthorityError::Overflow),
            _ => Err(AudioAuthorityError::CoordinateMismatch),
        }
    }
    pub(crate) fn origin(self) -> Result<ClockPoint, AudioAuthorityError> {
        self.point(match self {
            Self::SourceCounter { frequency, .. } => NativeOutputCoordinate::Counter {
                position: 0,
                frequency,
            },
            _ => NativeOutputCoordinate::StreamFrame { frame: 0 },
        })
    }
}

/// One original pair and its optional native units, retained in the same queue.
/// Metadata alone does not prove a native observation or generated source range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioAuthorityObservation {
    pair: ClockPair,
    coordinate: Option<NativeOutputCoordinate>,
}
impl AudioAuthorityObservation {
    pub const fn pair(self) -> ClockPair {
        self.pair
    }
    pub const fn coordinate(self) -> Option<NativeOutputCoordinate> {
        self.coordinate
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
    CoordinateModeMismatch,
    CoordinateMismatch,
    CoordinateMappingUnavailable,
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

/// Snapshot equality compares mathematical duration, independent of its assembly.
#[derive(Clone, Copy, Debug)]
struct ExactCoordinate(InterpolatedTargetTime);
impl PartialEq for ExactCoordinate {
    fn eq(&self, other: &Self) -> bool {
        self.0.compare_interpolated_time(other.0) == std::cmp::Ordering::Equal
    }
}
impl Eq for ExactCoordinate {}

/// Exact semantic state relevant to preparing and committing one operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PreparationState {
    revision: u64,
    config: AudioAuthorityConfig,
    epoch: AudioAuthorityEpoch,
    first: Option<AudioAuthorityObservation>,
    latest: Option<AudioAuthorityObservation>,
    coordinate_basis: Option<AudioAuthorityCoordinateBasis>,
    history_len: usize,
    acquired: Option<ClockPoint>,
    closed: Option<ClockPoint>,
    input_host: Option<ClockPoint>,
    operation: Option<ClockPoint>,
    presentation: Option<ClockPoint>,
    coordinate_operation: Option<ExactCoordinate>,
    coordinate_presentation: Option<ExactCoordinate>,
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
    /// The original input maps before this epoch's playable logical timeline.
    pub fn is_before_origin(&self) -> bool {
        self.output.timestamp < self.state.epoch.logical_origin.timestamp
    }
}

/// Frozen mathematical HOST mapping in original native units. Its unknown quality
/// grants no native observation, generated/submitted range or gameplay permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoordinateInputMapper {
    basis: AudioAuthorityCoordinateBasis,
    epoch: AudioAuthorityEpoch,
    left: AudioAuthorityObservation,
    right: AudioAuthorityObservation,
    latest: AudioAuthorityObservation,
    validity: ClockInterval,
    extrapolation: ExtrapolationPolicy,
    max_input_ahead: Duration,
}
impl CoordinateInputMapper {
    fn units(observation: AudioAuthorityObservation) -> Result<u64, AudioAuthorityError> {
        match observation.coordinate {
            Some(NativeOutputCoordinate::Counter { position, .. }) => Ok(position),
            Some(NativeOutputCoordinate::StreamFrame { frame }) => Ok(frame),
            None => Err(AudioAuthorityError::CoordinateMismatch),
        }
    }
    fn descriptor(&self) -> Result<(TargetTime, u64, ClockPoint), AudioAuthorityError> {
        self.basis.descriptor()
    }
    fn query_at(
        &self,
        original: ClockPoint,
    ) -> Result<Option<InterpolatedTargetTime>, AudioAuthorityError> {
        if original.domain != self.epoch.host_domain {
            return Err(AudioAuthorityError::DomainMismatch);
        }
        if original.timestamp < self.validity.start || original.timestamp > self.validity.end {
            return Ok(None);
        }
        let (base, frequency, _) = self.descriptor()?;
        let query = self.basis.interpolate(self.left, self.right, original)?;
        if original.timestamp > self.latest.pair.target.timestamp
            && query
                .compare_native_time_with_offset(
                    base,
                    Self::units(self.latest)?,
                    frequency,
                    self.max_input_ahead.as_nanos(),
                )
                .map_err(|_| AudioAuthorityError::Overflow)?
                == std::cmp::Ordering::Greater
        {
            return Ok(None);
        }
        Ok(Some(query))
    }
    fn raw_at(&self, query: InterpolatedTargetTime) -> Result<ClockPoint, AudioAuthorityError> {
        query
            .point(self.descriptor()?.2)
            .map_err(|_| AudioAuthorityError::Overflow)
    }
}
impl ClockMapper for CoordinateInputMapper {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        if to != self.epoch.logical_origin.domain {
            return None;
        }
        let query = self.query_at(from).ok()??;
        AudioAuthority::logical_in_epoch(self.epoch, self.raw_at(query).ok()?)
            .ok()
            .map(|point| point.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}

/// Read-only preparation of an acquired original input. This token freezes exact
/// mathematical correspondence, not native qualification or a gameplay commit.
#[derive(Clone, Copy, Debug)]
pub struct PreparedCoordinateInput {
    state: PreparationState,
    original: ClockPoint,
    now: ClockPoint,
    raw_output: ClockPoint,
    output: ClockPoint,
    query: InterpolatedTargetTime,
    mapper: CoordinateInputMapper,
    before_origin: bool,
}
impl PreparedCoordinateInput {
    pub const fn original(&self) -> ClockPoint {
        self.original
    }
    pub const fn epoch(&self) -> u64 {
        self.state.epoch.id
    }
    pub const fn raw_output(&self) -> ClockPoint {
        self.raw_output
    }
    pub const fn output(&self) -> ClockPoint {
        self.output
    }
    pub const fn mapper(&self) -> &CoordinateInputMapper {
        &self.mapper
    }
    pub fn is_before_origin(&self) -> bool {
        self.before_origin
    }
    pub const fn query(&self) -> InterpolatedTargetTime {
        self.query
    }
}

/// Original observed coordinate covered by an entirely drained acquired prefix.
/// The exact query retains presentation precision beside its rounded output.
#[derive(Clone, Copy, Debug)]
pub struct PreparedCoordinateFrontier {
    state: PreparationState,
    now: ClockPoint,
    host: ClockPoint,
    observation: AudioAuthorityObservation,
    query: InterpolatedTargetTime,
    output: ClockPoint,
    advance: Option<ClockPoint>,
}
impl PreparedCoordinateFrontier {
    pub const fn host(&self) -> ClockPoint {
        self.host
    }
    pub const fn observed_host(&self) -> ClockPoint {
        self.observation.pair.target
    }
    pub const fn observation(&self) -> AudioAuthorityObservation {
        self.observation
    }
    pub const fn query(&self) -> InterpolatedTargetTime {
        self.query
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

/// Mathematical native control cut; this token does not acknowledge a producer.
#[derive(Clone, Copy, Debug)]
pub struct PreparedCoordinateControlCutoff {
    state: PreparationState,
    epoch: u64,
    query: InterpolatedTargetTime,
    raw_output: ClockPoint,
    host_cutoff: ClockPoint,
    now: ClockPoint,
    output: ClockPoint,
    resume: bool,
}
impl PreparedCoordinateControlCutoff {
    pub const fn is_resume(&self) -> bool {
        self.resume
    }
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
    pub const fn query(&self) -> InterpolatedTargetTime {
        self.query
    }
}

/// Actual coordinate presentation/prefix closure with no Runtime operation grant.
#[derive(Clone, Copy, Debug)]
pub struct PreparedCoordinateHeldFrontier {
    frontier: PreparedCoordinateFrontier,
}
impl PreparedCoordinateHeldFrontier {
    pub const fn host(&self) -> ClockPoint {
        self.frontier.host()
    }
    pub const fn observed_host(&self) -> ClockPoint {
        self.frontier.observed_host()
    }
    pub const fn output(&self) -> ClockPoint {
        self.frontier.output()
    }
    pub const fn query(&self) -> InterpolatedTargetTime {
        self.frontier.query()
    }
    pub const fn epoch(&self) -> u64 {
        self.frontier.epoch()
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
    history: VecDeque<AudioAuthorityObservation>,
    coordinate_basis: Option<AudioAuthorityCoordinateBasis>,
    revision: u64,
    acquired: Option<ClockPoint>,
    closed: Option<ClockPoint>,
    input_host: Option<ClockPoint>,
    operation: Option<ClockPoint>,
    presentation: Option<ClockPoint>,
    coordinate_operation: Option<InterpolatedTargetTime>,
    coordinate_presentation: Option<InterpolatedTargetTime>,
}
impl AudioAuthority {
    pub fn new(
        config: AudioAuthorityConfig,
        epoch: AudioAuthorityEpoch,
    ) -> Result<Self, AudioAuthorityError> {
        Self::new_inner(config, epoch, None)
    }
    /// Cold opt-in preserves the original basis, including any fractional start.
    /// Mapping operations remain unavailable until an exact coordinate mapper exists.
    pub fn new_with_coordinates(
        config: AudioAuthorityConfig,
        epoch: AudioAuthorityEpoch,
        basis: AudioAuthorityCoordinateBasis,
    ) -> Result<Self, AudioAuthorityError> {
        Self::new_inner(config, epoch, Some(basis))
    }
    fn new_inner(
        config: AudioAuthorityConfig,
        epoch: AudioAuthorityEpoch,
        coordinate_basis: Option<AudioAuthorityCoordinateBasis>,
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
        if let Some(basis) = coordinate_basis {
            if matches!(
                basis,
                AudioAuthorityCoordinateBasis::SourceCounter { frequency: 0, .. }
            ) {
                return Err(AudioAuthorityError::InvalidConfig);
            }
            let origin = basis.origin()?;
            if origin.domain != epoch.stream_origin.domain {
                return Err(AudioAuthorityError::DomainMismatch);
            }
            if origin != epoch.stream_origin {
                return Err(AudioAuthorityError::CoordinateMismatch);
            }
        }
        let mut history = VecDeque::new();
        history
            .try_reserve_exact(config.history_capacity)
            .map_err(|_| AudioAuthorityError::AllocationFailed)?;
        Ok(Self {
            config,
            epoch,
            history,
            coordinate_basis,
            revision: 0,
            acquired: None,
            closed: None,
            input_host: None,
            operation: None,
            presentation: None,
            coordinate_operation: None,
            coordinate_presentation: None,
        })
    }
    pub const fn config(&self) -> AudioAuthorityConfig {
        self.config
    }
    pub const fn epoch(&self) -> AudioAuthorityEpoch {
        self.epoch
    }
    pub fn latest_observation(&self) -> Option<ClockPair> {
        self.history.back().map(|entry| entry.pair)
    }
    pub const fn coordinate_basis(&self) -> Option<AudioAuthorityCoordinateBasis> {
        self.coordinate_basis
    }
    pub fn latest_coordinate_observation(&self) -> Option<AudioAuthorityObservation> {
        self.history
            .back()
            .copied()
            .filter(|entry| entry.coordinate.is_some())
    }
    /// Conservative exact lower bound for future legal input correspondence.
    /// This grants no retirement or native permission: outstanding controls,
    /// practice receipts and other consumers require their own retained pins.
    pub fn future_input_retention_query(
        &self,
    ) -> Result<InterpolatedTargetTime, AudioAuthorityError> {
        let basis = self
            .coordinate_basis
            .ok_or(AudioAuthorityError::CoordinateModeMismatch)?;
        if let Some(closed) = self.closed {
            if let Some(observation) = self
                .history
                .iter()
                .rev()
                .find(|entry| entry.pair.target.timestamp <= closed.timestamp)
            {
                return basis.query(*observation);
            }
        }
        if self.config.input_extrapolation == ExtrapolationPolicy::Forbid {
            if let Some(first) = self.history.front() {
                return basis.query(*first);
            }
        }
        // Before-first extrapolation keeps the immutable creation bound, even
        // when the configured before allowance is zero.
        let (base, frequency, _) = basis.descriptor()?;
        InterpolatedTargetTime::new(base, 0, 0, frequency, 0, 1)
            .map_err(|_| AudioAuthorityError::Overflow)
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
    pub const fn committed_coordinate_operation(&self) -> Option<InterpolatedTargetTime> {
        self.coordinate_operation
    }
    pub const fn committed_coordinate_presentation(&self) -> Option<InterpolatedTargetTime> {
        self.coordinate_presentation
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
        if self.coordinate_basis.is_some() {
            return Err(AudioAuthorityError::CoordinateModeMismatch);
        }
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
                .is_some_and(|closed| self.history[1].pair.target.timestamp <= closed.timestamp)
        {
            return Err(AudioAuthorityError::HistoryCapacity);
        }
        let revision = self.next_revision()?;
        if retire {
            self.history.pop_front();
        }
        self.history.push_back(AudioAuthorityObservation {
            pair,
            coordinate: None,
        });
        self.revision = revision;
        Ok(ObservationAdmission::Retained)
    }

    /// Retains consistent original native units beside the unchanged raw pair.
    /// Progress in native units can share a rounded nanosecond with the last pair.
    /// This does not authorize presentation or validate native acquisition evidence.
    pub fn observe_coordinate(
        &mut self,
        epoch: u64,
        pair: ClockPair,
        coordinate: NativeOutputCoordinate,
    ) -> Result<ObservationAdmission, AudioAuthorityError> {
        let basis = self
            .coordinate_basis
            .ok_or(AudioAuthorityError::CoordinateModeMismatch)?;
        if epoch != self.epoch.id {
            return Err(AudioAuthorityError::WrongEpoch);
        }
        if pair.source.domain != self.epoch.stream_origin.domain
            || pair.target.domain != self.epoch.host_domain
        {
            return Err(AudioAuthorityError::DomainMismatch);
        }
        if basis.point(coordinate)? != pair.source {
            return Err(AudioAuthorityError::CoordinateMismatch);
        }
        self.logical(pair.source)?;
        let query = basis.query(AudioAuthorityObservation {
            pair,
            coordinate: Some(coordinate),
        })?;
        if self
            .coordinate_presentation
            .is_some_and(|last| query.compare_interpolated_time(last) == std::cmp::Ordering::Less)
        {
            return Err(AudioAuthorityError::ObservationRegression);
        }
        if let Some(previous) = self.history.back() {
            // Kind and frequency were checked against the immutable descriptor.
            let position = |coordinate| match coordinate {
                NativeOutputCoordinate::Counter { position, .. } => position,
                NativeOutputCoordinate::StreamFrame { frame } => frame,
            };
            let previous_coordinate = previous
                .coordinate
                .ok_or(AudioAuthorityError::CoordinateModeMismatch)?;
            let current = position(coordinate);
            let old = position(previous_coordinate);
            if current < old
                || pair.source.timestamp < previous.pair.source.timestamp
                || pair.target.timestamp < previous.pair.target.timestamp
            {
                return Err(AudioAuthorityError::ObservationRegression);
            }
            if current == old {
                return Ok(ObservationAdmission::Unchanged);
            }
            if pair.target.timestamp == previous.pair.target.timestamp {
                return Err(AudioAuthorityError::ObservationRegression);
            }
            // No Affine mapper is created here: its rounded endpoints may be equal.
        }
        let retire = self.history.len() == self.config.history_capacity;
        if retire
            && !self
                .closed
                .is_some_and(|closed| self.history[1].pair.target.timestamp <= closed.timestamp)
        {
            return Err(AudioAuthorityError::HistoryCapacity);
        }
        let revision = self.next_revision()?;
        if retire {
            self.history.pop_front();
        }
        self.history.push_back(AudioAuthorityObservation {
            pair,
            coordinate: Some(coordinate),
        });
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
        self.require_scalar_mapping()?;
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
        let first = self.history[0].pair;
        let latest = self
            .history
            .back()
            .expect("two real anchors were checked")
            .pair;
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
                self.history[1].pair,
                ClockInterval {
                    start,
                    end: self.history[1].pair.target.timestamp,
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
            let left = self.history[self.history.len() - 2].pair;
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
                .find(|&index| original.timestamp <= self.history[index + 1].pair.target.timestamp)
                .expect("point lies within retained real anchors");
            let left = self.history[index].pair;
            let right = self.history[index + 1].pair;
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
        if output.timestamp >= self.epoch.logical_origin.timestamp
            && self
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

    /// Prepare exact mathematical correspondence for an acquired original input.
    /// This does not consume input or grant native/playback qualification.
    pub fn prepare_coordinate_input(
        &self,
        original: ClockPoint,
        now: ClockPoint,
    ) -> Result<Option<PreparedCoordinateInput>, AudioAuthorityError> {
        let basis = self
            .coordinate_basis
            .ok_or(AudioAuthorityError::CoordinateModeMismatch)?;
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
        let (left, right, validity, extrapolation) = if original.timestamp
            < first.pair.target.timestamp
        {
            let ExtrapolationPolicy::Bounded { before, .. } = self.config.input_extrapolation
            else {
                return Err(AudioAuthorityError::HistoryExpired);
            };
            let start = add(first.pair.target.timestamp, -i128::from(before.as_nanos()))?;
            if original.timestamp < start {
                return Err(AudioAuthorityError::HistoryExpired);
            }
            (
                first,
                self.history[1],
                ClockInterval {
                    start,
                    end: self.history[1].pair.target.timestamp,
                },
                ExtrapolationPolicy::Bounded {
                    before,
                    after: Duration::ZERO,
                },
            )
        } else if original.timestamp > latest.pair.target.timestamp {
            let ExtrapolationPolicy::Bounded { after, .. } = self.config.input_extrapolation else {
                return Ok(None);
            };
            let end = add(latest.pair.target.timestamp, i128::from(after.as_nanos()))?;
            if original.timestamp > end {
                return Ok(None);
            }
            let left = self.history[self.history.len() - 2];
            (
                left,
                latest,
                ClockInterval {
                    start: left.pair.target.timestamp,
                    end,
                },
                ExtrapolationPolicy::Bounded {
                    before: Duration::ZERO,
                    after,
                },
            )
        } else {
            let index = (0..self.history.len() - 1)
                .find(|&index| original.timestamp <= self.history[index + 1].pair.target.timestamp)
                .expect("point lies within retained real anchors");
            let left = self.history[index];
            let right = self.history[index + 1];
            (
                left,
                right,
                ClockInterval {
                    start: left.pair.target.timestamp,
                    end: right.pair.target.timestamp,
                },
                ExtrapolationPolicy::Forbid,
            )
        };
        let mapper = CoordinateInputMapper {
            basis,
            epoch: self.epoch,
            left,
            right,
            latest,
            validity,
            extrapolation,
            max_input_ahead: self.config.max_input_ahead,
        };
        let Some(query) = mapper.query_at(original)? else {
            return Ok(None);
        };
        let raw_output = mapper.raw_at(query)?;
        let output = self.logical(raw_output)?;
        let before_origin =
            query.compare_target_time(basis.descriptor()?.0) == std::cmp::Ordering::Less;
        if !before_origin
            && self.coordinate_operation.is_some_and(|last| {
                query.compare_interpolated_time(last) == std::cmp::Ordering::Less
            })
        {
            return Err(AudioAuthorityError::OperationRegression);
        }
        self.next_revision()?;
        Ok(Some(PreparedCoordinateInput {
            state: self.state(),
            original,
            now,
            raw_output,
            output,
            query,
            mapper,
            before_origin,
        }))
    }

    /// Recheck current acquisition/freshness without consuming or modifying input.
    /// A later validation instant is allowed while the same correspondence remains valid.
    pub fn validate_coordinate_input(
        &self,
        prepared: &PreparedCoordinateInput,
        now: ClockPoint,
    ) -> Result<(), AudioAuthorityError> {
        self.validate_host(now)?;
        if prepared.state != self.state() || now.timestamp < prepared.now.timestamp {
            return Err(AudioAuthorityError::StalePreparation);
        }
        let Some(current) = self.prepare_coordinate_input(prepared.original, now)? else {
            return Err(AudioAuthorityError::StalePreparation);
        };
        if current.original != prepared.original
            || current.mapper != prepared.mapper
            || current.raw_output != prepared.raw_output
            || current.output != prepared.output
            || current.before_origin != prepared.before_origin
            || current.query.compare_interpolated_time(prepared.query) != std::cmp::Ordering::Equal
        {
            return Err(AudioAuthorityError::StalePreparation);
        }
        Ok(())
    }

    /// Commit after the caller's real phase-aware Runtime operation.
    pub fn commit_coordinate_input(
        &mut self,
        prepared: PreparedCoordinateInput,
        now: ClockPoint,
    ) -> Result<(), AudioAuthorityError> {
        if prepared.is_before_origin() {
            return Err(AudioAuthorityError::OperationRegression);
        }
        self.validate_coordinate_input(&prepared, now)?;
        let revision = self.next_revision()?;
        self.input_host = Some(prepared.original);
        self.operation = Some(prepared.output);
        self.coordinate_operation = Some(prepared.query);
        self.revision = revision;
        Ok(())
    }

    /// Retire pre-origin input without granting gameplay or presentation progress.
    pub fn commit_discarded_coordinate_input(
        &mut self,
        prepared: PreparedCoordinateInput,
        now: ClockPoint,
    ) -> Result<(), AudioAuthorityError> {
        if !prepared.is_before_origin() {
            return Err(AudioAuthorityError::StalePreparation);
        }
        self.validate_coordinate_input(&prepared, now)?;
        let revision = self.next_revision()?;
        self.input_host = Some(prepared.original);
        self.revision = revision;
        Ok(())
    }

    /// Only retained original presentation covered by a drained prefix advances.
    /// HOST now is used for freshness and never extrapolated into presentation.
    pub fn prepare_coordinate_frontier(
        &self,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<Option<PreparedCoordinateFrontier>, AudioAuthorityError> {
        let basis = self
            .coordinate_basis
            .ok_or(AudioAuthorityError::CoordinateModeMismatch)?;
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
        let Some(observation) = self
            .history
            .iter()
            .rev()
            .copied()
            .find(|entry| entry.pair.target.timestamp <= host.timestamp)
        else {
            return Ok(None);
        };
        if !self.pair_is_fresh(observation.pair, now) {
            return Ok(None);
        }
        let query = basis.query(observation)?;
        if query.compare_target_time(basis.descriptor()?.0) == std::cmp::Ordering::Less {
            return Ok(None);
        }
        let output = self.logical(observation.pair.source)?;
        if self
            .coordinate_presentation
            .is_some_and(|last| query.compare_interpolated_time(last) == std::cmp::Ordering::Less)
        {
            return Err(AudioAuthorityError::ObservationRegression);
        }
        if self.closed == Some(host)
            && self.coordinate_presentation.is_some_and(|last| {
                query.compare_interpolated_time(last) == std::cmp::Ordering::Equal
            })
        {
            return Ok(None);
        }
        let advance = self
            .coordinate_operation
            .is_none_or(|last| query.compare_interpolated_time(last) == std::cmp::Ordering::Greater)
            .then_some(output);
        self.next_revision()?;
        Ok(Some(PreparedCoordinateFrontier {
            state: self.state(),
            now,
            host,
            observation,
            query,
            output,
            advance,
        }))
    }

    pub fn commit_coordinate_frontier(
        &mut self,
        prepared: PreparedCoordinateFrontier,
        now: ClockPoint,
        merger: &mut InputMerger,
    ) -> Result<(), AudioAuthorityError> {
        self.commit_coordinate_frontier_inner(prepared, now, merger, true)
    }

    fn commit_coordinate_frontier_inner(
        &mut self,
        prepared: PreparedCoordinateFrontier,
        now: ClockPoint,
        merger: &mut InputMerger,
        advance_operation: bool,
    ) -> Result<(), AudioAuthorityError> {
        self.validate_host(now)?;
        if prepared.state != self.state() || now.timestamp < prepared.now.timestamp {
            return Err(AudioAuthorityError::StalePreparation);
        }
        let Some(current) = self.prepare_coordinate_frontier(now, merger)? else {
            return Err(AudioAuthorityError::StalePreparation);
        };
        if current.host != prepared.host
            || current.observation != prepared.observation
            || current.output != prepared.output
            || current.advance != prepared.advance
            || current.query.compare_interpolated_time(prepared.query) != std::cmp::Ordering::Equal
        {
            return Err(AudioAuthorityError::StalePreparation);
        }
        let revision = self.next_revision()?;
        merger.commit(prepared.host)?;
        self.closed = Some(prepared.host);
        self.presentation = Some(prepared.output);
        self.coordinate_presentation = Some(prepared.query);
        if let Some(output) = prepared.advance.filter(|_| advance_operation) {
            self.operation = Some(output);
            self.coordinate_operation = Some(prepared.query);
        }
        self.revision = revision;
        Ok(())
    }

    /// Locate the first integer HOST at or beyond a cut in original native units.
    /// Both enclosing associations must already be retained; no extrapolation.
    pub fn presented_coordinate_boundary_host(
        &self,
        query: &InterpolatedTargetTime,
        now: ClockPoint,
    ) -> Result<Option<ClockPoint>, AudioAuthorityError> {
        let basis = self
            .coordinate_basis
            .ok_or(AudioAuthorityError::CoordinateModeMismatch)?;
        self.validate_host(now)?;
        if query.compare_target_time(basis.descriptor()?.0) == std::cmp::Ordering::Less {
            return Err(AudioAuthorityError::ObservationRegression);
        }
        let Some(first) = self.history.front().copied() else {
            return Ok(None);
        };
        if query.compare_interpolated_time(basis.query(first)?) == std::cmp::Ordering::Less {
            return Err(AudioAuthorityError::HistoryExpired);
        }
        if self.history.len() < 2 || !self.fresh(now)? {
            return Ok(None);
        }
        let mut enclosing = None;
        for index in 0..self.history.len() - 1 {
            let right = self.history[index + 1];
            if query.compare_interpolated_time(basis.query(right)?) != std::cmp::Ordering::Greater {
                enclosing = Some((self.history[index], right));
                break;
            }
        }
        let Some((left, right)) = enclosing else {
            return Ok(None);
        };
        if !self.pair_is_fresh(right.pair, now) {
            return Ok(None);
        }
        let mapper = CoordinateInputMapper {
            basis,
            epoch: self.epoch,
            left,
            right,
            latest: right,
            validity: ClockInterval {
                start: left.pair.target.timestamp,
                end: right.pair.target.timestamp,
            },
            extrapolation: ExtrapolationPolicy::Forbid,
            max_input_ahead: Duration::ZERO,
        };
        let mut lower = 0u64;
        let mut upper = u64::try_from(delta(
            right.pair.target.timestamp,
            left.pair.target.timestamp,
        ))
        .map_err(|_| AudioAuthorityError::Overflow)?;
        while lower < upper {
            let middle = lower + (upper - lower) / 2;
            let host = ClockPoint {
                domain: self.epoch.host_domain,
                timestamp: add(left.pair.target.timestamp, i128::from(middle))?,
            };
            let at = mapper
                .query_at(host)?
                .ok_or(AudioAuthorityError::CoordinateMappingUnavailable)?;
            if at.compare_interpolated_time(*query) == std::cmp::Ordering::Less {
                lower = middle + 1;
            } else {
                upper = middle;
            }
        }
        Ok(Some(ClockPoint {
            domain: self.epoch.host_domain,
            timestamp: add(left.pair.target.timestamp, i128::from(lower))?,
        }))
    }

    pub fn prepare_coordinate_control_cutoff(
        &self,
        epoch: u64,
        query: InterpolatedTargetTime,
        host_cutoff: ClockPoint,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<Option<PreparedCoordinateControlCutoff>, AudioAuthorityError> {
        self.prepare_coordinate_control_inner(epoch, query, host_cutoff, now, merger, false)
    }

    pub fn prepare_coordinate_resume_control_cutoff(
        &self,
        epoch: u64,
        query: InterpolatedTargetTime,
        host_cutoff: ClockPoint,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<Option<PreparedCoordinateControlCutoff>, AudioAuthorityError> {
        self.prepare_coordinate_control_inner(epoch, query, host_cutoff, now, merger, true)
    }

    fn prepare_coordinate_control_inner(
        &self,
        epoch: u64,
        query: InterpolatedTargetTime,
        host_cutoff: ClockPoint,
        now: ClockPoint,
        merger: &InputMerger,
        resume: bool,
    ) -> Result<Option<PreparedCoordinateControlCutoff>, AudioAuthorityError> {
        let basis = self
            .coordinate_basis
            .ok_or(AudioAuthorityError::CoordinateModeMismatch)?;
        if epoch != self.epoch.id {
            return Err(AudioAuthorityError::WrongEpoch);
        }
        self.validate_host(host_cutoff)?;
        self.validate_host(now)?;
        let raw_output = query
            .point(basis.descriptor()?.2)
            .map_err(|_| AudioAuthorityError::Overflow)?;
        let output = self.logical(raw_output)?;
        if query.compare_target_time(basis.descriptor()?.0) == std::cmp::Ordering::Less {
            return Err(AudioAuthorityError::ObservationRegression);
        }
        if self
            .coordinate_operation
            .is_some_and(|last| query.compare_interpolated_time(last) == std::cmp::Ordering::Less)
            || self
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
        if prefix.timestamp > now.timestamp || host_cutoff.timestamp > prefix.timestamp {
            return Ok(None);
        }
        let Some(qualified) = self.presented_coordinate_boundary_host(&query, now)? else {
            return Ok(None);
        };
        if qualified != host_cutoff {
            return Err(AudioAuthorityError::CoordinateMismatch);
        }
        if merger.peek_ready(prefix)?.is_some_and(|event| {
            event.meta().timestamp < host_cutoff.timestamp
                || (!resume && event.meta().timestamp == host_cutoff.timestamp)
        }) {
            return Ok(None);
        }
        self.next_revision()?;
        Ok(Some(PreparedCoordinateControlCutoff {
            state: self.state(),
            epoch,
            query,
            raw_output,
            host_cutoff,
            now,
            output,
            resume,
        }))
    }

    pub fn validate_coordinate_control_cutoff(
        &self,
        prepared: &PreparedCoordinateControlCutoff,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<(), AudioAuthorityError> {
        self.validate_host(now)?;
        if prepared.state != self.state() || now.timestamp < prepared.now.timestamp {
            return Err(AudioAuthorityError::StalePreparation);
        }
        let Some(current) = self.prepare_coordinate_control_inner(
            prepared.epoch,
            prepared.query,
            prepared.host_cutoff,
            now,
            merger,
            prepared.resume,
        )?
        else {
            return Err(AudioAuthorityError::StalePreparation);
        };
        if current.raw_output != prepared.raw_output
            || current.output != prepared.output
            || current.query.compare_interpolated_time(prepared.query) != std::cmp::Ordering::Equal
        {
            return Err(AudioAuthorityError::StalePreparation);
        }
        Ok(())
    }

    /// Commit only the caller's actual accepted phase control operation.
    pub fn commit_coordinate_control_cutoff(
        &mut self,
        prepared: PreparedCoordinateControlCutoff,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<(), AudioAuthorityError> {
        self.validate_coordinate_control_cutoff(&prepared, now, merger)?;
        let revision = self.next_revision()?;
        self.operation = Some(prepared.output);
        self.coordinate_operation = Some(prepared.query);
        self.revision = revision;
        Ok(())
    }

    pub fn prepare_coordinate_held_frontier(
        &self,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<Option<PreparedCoordinateHeldFrontier>, AudioAuthorityError> {
        Ok(self
            .prepare_coordinate_frontier(now, merger)?
            .map(|frontier| PreparedCoordinateHeldFrontier { frontier }))
    }

    /// Held presentation closes the prefix without changing either operation watermark.
    pub fn commit_coordinate_held_frontier(
        &mut self,
        prepared: PreparedCoordinateHeldFrontier,
        now: ClockPoint,
        merger: &mut InputMerger,
    ) -> Result<(), AudioAuthorityError> {
        self.commit_coordinate_frontier_inner(prepared.frontier, now, merger, false)
    }

    /// Commit the actual Runtime operation once; post-report observer faults do not undo it.
    pub fn commit_input(&mut self, prepared: PreparedInput) -> Result<(), AudioAuthorityError> {
        if prepared.is_before_origin() {
            return Err(AudioAuthorityError::OperationRegression);
        }
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

    /// Retire an original pre-origin input without granting an operation or presentation.
    pub fn commit_discarded_input(
        &mut self,
        prepared: PreparedInput,
    ) -> Result<(), AudioAuthorityError> {
        if !prepared.is_before_origin()
            || prepared.state != self.state()
            || self.prepare_input(prepared.original, prepared.now)? != Some(prepared)
        {
            return Err(AudioAuthorityError::StalePreparation);
        }
        let revision = self.next_revision()?;
        self.input_host = Some(prepared.original);
        self.revision = revision;
        Ok(())
    }

    /// Only a real observation covered by a fully drained HOST prefix grants presentation.
    pub fn prepare_frontier(
        &self,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<Option<PreparedFrontier>, AudioAuthorityError> {
        self.require_scalar_mapping()?;
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
            .map(|entry| &entry.pair)
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
        self.require_scalar_mapping()?;
        self.validate_host(now)?;
        self.logical(raw)?;
        if raw.timestamp < self.epoch.stream_origin.timestamp {
            return Err(AudioAuthorityError::ObservationRegression);
        }
        let Some(first) = self.history.front() else {
            return Ok(None);
        };
        if raw.timestamp < first.pair.source.timestamp {
            return Err(AudioAuthorityError::HistoryExpired);
        }
        if self.history.len() < 2 || !self.fresh(now)? {
            return Ok(None);
        }
        let Some(index) = (0..self.history.len() - 1)
            .find(|&i| raw.timestamp <= self.history[i + 1].pair.source.timestamp)
        else {
            return Ok(None);
        };
        let left = self.history[index].pair;
        let right = self.history[index + 1].pair;
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
        self.require_scalar_mapping()?;
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
        self.require_scalar_mapping()?;
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
        self.require_scalar_mapping()?;
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
        self.history
            .extend(prepared.pairs.map(|pair| AudioAuthorityObservation {
                pair,
                coordinate: None,
            }));
        self.revision = revision;
        Ok(())
    }
    fn state(&self) -> PreparationState {
        PreparationState {
            revision: self.revision,
            config: self.config,
            epoch: self.epoch,
            first: self.history.front().copied(),
            latest: self.history.back().copied(),
            coordinate_basis: self.coordinate_basis,
            history_len: self.history.len(),
            acquired: self.acquired,
            closed: self.closed,
            input_host: self.input_host,
            operation: self.operation,
            presentation: self.presentation,
            coordinate_operation: self.coordinate_operation.map(ExactCoordinate),
            coordinate_presentation: self.coordinate_presentation.map(ExactCoordinate),
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
        self.require_scalar_mapping()?;
        self.logical(raw)
    }
    fn require_scalar_mapping(&self) -> Result<(), AudioAuthorityError> {
        if self.coordinate_basis.is_some() {
            Err(AudioAuthorityError::CoordinateMappingUnavailable)
        } else {
            Ok(())
        }
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
            .any(|entry| self.pair_is_fresh(entry.pair, now)))
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

#[cfg(test)]
#[path = "audio_authority_coordinate_fixtures.rs"]
mod audio_authority_coordinate_fixtures;

#[cfg(test)]
#[path = "audio_authority_coordinate_input_fixtures.rs"]
mod audio_authority_coordinate_input_fixtures;

#[cfg(test)]
#[path = "audio_authority_coordinate_commit_fixtures.rs"]
mod audio_authority_coordinate_commit_fixtures;

#[cfg(test)]
#[path = "audio_authority_coordinate_control_fixtures.rs"]
mod audio_authority_coordinate_control_fixtures;

#[cfg(test)]
#[path = "audio_authority_retention_fixtures.rs"]
mod audio_authority_retention_fixtures;
