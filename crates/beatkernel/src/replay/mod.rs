//! Ordered normalized gameplay replay with complete reusable checkpoints.
//!
//! Logical judge state is reconstructed forward; audio device/output clocks are
//! outside this recording. Custom snapshot support must be explicitly supplied.

use crate::judge::snapshot::{Encoder, hash};
use crate::{
    input::GameInputEvent,
    judge::{JudgeEngine, JudgeError, JudgeEvent, JudgeSnapshot, SnapshotError},
    runtime::RuntimeReport,
    time::{ClockDomainId, Timestamp},
};

/// Canonical replay schema version.
pub const REPLAY_VERSION: u32 = 1;

/// Application-provided durable identities and deterministic configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayHeader {
    /// Must equal [`REPLAY_VERSION`].
    pub version: u32,
    /// Durable application chart/content identity.
    pub chart_identity: Vec<u8>,
    /// Rule implementations, policy versions and adapter/profile identity.
    pub rules_identity: Vec<u8>,
    /// Canonical application options, including any rule-specific configuration.
    pub options: Vec<u8>,
    /// Seed used by the application to construct deterministic rules.
    pub seed: u64,
    /// Domain of already-normalized event metadata.
    pub normalized_clock: ClockDomainId,
}

/// Recorded operation; advances retain exact timeout/result emission times.
#[derive(Clone, Debug, PartialEq)]
pub enum ReplayOperation {
    /// Already-bound event with unchanged physical payload and provenance.
    Input(GameInputEvent),
    /// Explicit caller advancement without input.
    Advance,
}

/// Globally ordered operation at explicit, unoffset mapped song time.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplayRecord {
    /// Strict ordinal including report/binding fanout at equal song times.
    pub ordinal: u64,
    /// Song time before the judge profile applies its offset once.
    pub song_time: Timestamp,
    /// Bound input or explicit advance.
    pub operation: ReplayOperation,
}

/// Collects the live judge's accepted operations without executing another judge.
pub struct ReplayRecorder {
    header: ReplayHeader,
    records: Vec<ReplayRecord>,
}
impl ReplayRecorder {
    /// Starts an empty recording in an explicit normalized input domain.
    pub fn new(header: ReplayHeader) -> Result<Self, ReplayError> {
        if header.version != REPLAY_VERSION {
            return Err(ReplayError::UnsupportedVersion(header.version));
        }
        Ok(Self {
            header,
            records: Vec::new(),
        })
    }
    /// Appends a live operation's successfully admitted input prefix or advance.
    ///
    /// Call once for each report, in runtime operation order. Audio admission
    /// failures do not change the accepted judge log. Validation is atomic across
    /// the report; a reported judge error still permits recording accepted inputs.
    pub fn record_report(&mut self, report: &RuntimeReport) -> Result<(), ReplayError> {
        let advance = report.input.is_none() && report.judge_error.is_none();
        let count = report.bound_inputs.len() + usize::from(advance);
        if count == 0 {
            return Ok(());
        }
        if self
            .records
            .last()
            .is_some_and(|last| last.song_time > report.song_time)
        {
            return Err(ReplayError::NonMonotonicSongTime);
        }
        if report
            .bound_inputs
            .iter()
            .any(|input| input.physical.meta().clock_domain != self.header.normalized_clock)
        {
            return Err(ReplayError::ClockDomainMismatch);
        }
        let end = self
            .records
            .len()
            .checked_add(count)
            .ok_or(ReplayError::Overflow)?;
        u64::try_from(end).map_err(|_| ReplayError::Overflow)?;
        for input in &report.bound_inputs {
            self.records.push(ReplayRecord {
                ordinal: self.records.len() as u64,
                song_time: report.song_time,
                operation: ReplayOperation::Input(input.clone()),
            });
        }
        if advance {
            self.records.push(ReplayRecord {
                ordinal: self.records.len() as u64,
                song_time: report.song_time,
                operation: ReplayOperation::Advance,
            });
        }
        Ok(())
    }
    /// Ordered input/advance log for serialization or subsequent reconstruction.
    pub fn records(&self) -> &[ReplayRecord] {
        &self.records
    }
    /// Declared chart, rule, options and normalized clock identity.
    pub fn header(&self) -> &ReplayHeader {
        &self.header
    }
    /// Transfers ownership to the host serializer or ReplaySession constructor.
    pub fn into_parts(self) -> (ReplayHeader, Vec<ReplayRecord>) {
        (self.header, self.records)
    }
}

/// Reusable checkpoint with exact result prefix and recording identity.
pub struct ReplayCheckpoint {
    cursor: usize,
    results: Vec<JudgeEvent>,
    state: JudgeSnapshot,
    boundary_time: Option<Timestamp>,
}
impl ReplayCheckpoint {
    /// Number of operations reflected by this checkpoint.
    pub const fn cursor(&self) -> usize {
        self.cursor
    }
    /// Complete result prefix, including native input provenance.
    pub fn results(&self) -> &[JudgeEvent] {
        &self.results
    }
    /// Synthetic target advance, if this checkpoint captures an arbitrary seek.
    pub const fn boundary_time(&self) -> Option<Timestamp> {
        self.boundary_time
    }
    /// Exact effective judge time, including the profile offset.
    pub fn effective_song_time(&self) -> Option<Timestamp> {
        self.state.effective_song_time()
    }
}

/// Replay validation, chronology or complete-state checkpoint failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayError {
    /// Header schema is unsupported.
    UnsupportedVersion(u32),
    /// Initial engine has already accepted an operation.
    AlreadyStarted,
    /// Event is not in the header's declared normalized clock domain.
    ClockDomainMismatch,
    /// Operation ordinal is not its zero-based recording index.
    InvalidOrdinal,
    /// Recording song time regressed.
    NonMonotonicSongTime,
    /// New recording requires explicitly discarding the existing future.
    FutureExists,
    /// Replay has too many operations for an ordinal.
    Overflow,
    /// Judge rejected the unchanged operation.
    Judge(JudgeError),
    /// A complete reusable snapshot could not be obtained/restored.
    Snapshot(SnapshotError),
}
impl From<JudgeError> for ReplayError {
    fn from(value: JudgeError) -> Self {
        Self::Judge(value)
    }
}
impl From<SnapshotError> for ReplayError {
    fn from(value: SnapshotError) -> Self {
        Self::Snapshot(value)
    }
}
impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion(v) => write!(f, "unsupported replay version {v}"),
            Self::AlreadyStarted => {
                f.write_str("replay requires an engine before its first operation")
            }
            Self::ClockDomainMismatch => {
                f.write_str("replay event is not in the normalized clock domain")
            }
            Self::InvalidOrdinal => f.write_str("replay ordinal differs from acquisition order"),
            Self::NonMonotonicSongTime => f.write_str("replay song time moved backward"),
            Self::FutureExists => {
                f.write_str("fork the recording before adding input after a seek")
            }
            Self::Overflow => f.write_str("replay ordinal overflow"),
            Self::Judge(error) => error.fmt(f),
            Self::Snapshot(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for ReplayError {}

/// Single-owner deterministic recording/playback outside real-time callbacks.
///
/// This session owns the judge, preventing unrecorded mutable operations. Seek
/// selects recorded operations at/before the target, then advances to the exact
/// requested boundary. Boundary advances remain separate from the durable log.
pub struct ReplaySession {
    header: ReplayHeader,
    engine: JudgeEngine,
    records: Vec<ReplayRecord>,
    results: Vec<JudgeEvent>,
    cursor: usize,
    checkpoints: Vec<ReplayCheckpoint>,
    boundary_time: Option<Timestamp>,
}
impl ReplaySession {
    /// Starts from a pristine engine, checking complete checkpoint support.
    pub fn new(header: ReplayHeader, engine: JudgeEngine) -> Result<Self, ReplayError> {
        if header.version != REPLAY_VERSION {
            return Err(ReplayError::UnsupportedVersion(header.version));
        }
        if engine.effective_song_time().is_some() {
            return Err(ReplayError::AlreadyStarted);
        }
        let origin = ReplayCheckpoint {
            cursor: 0,
            results: Vec::new(),
            state: engine.snapshot()?,
            boundary_time: None,
        };
        Ok(Self {
            header,
            engine,
            records: Vec::new(),
            results: Vec::new(),
            cursor: 0,
            checkpoints: vec![origin],
            boundary_time: None,
        })
    }
    /// Loads an ordered recording by applying it through the existing judge.
    pub fn from_records(
        header: ReplayHeader,
        engine: JudgeEngine,
        records: impl IntoIterator<Item = ReplayRecord>,
    ) -> Result<Self, ReplayError> {
        let mut session = Self::new(header, engine)?;
        for record in records {
            session.append_record(record)?;
        }
        Ok(session)
    }
    /// Immutable application replay metadata.
    pub const fn header(&self) -> &ReplayHeader {
        &self.header
    }
    /// Unchanged globally ordered recording, including explicit advances.
    pub fn records(&self) -> &[ReplayRecord] {
        &self.records
    }
    /// Current reconstructed result prefix.
    pub fn results(&self) -> &[JudgeEvent] {
        &self.results
    }
    /// Immutable logical judge state.
    pub const fn engine(&self) -> &JudgeEngine {
        &self.engine
    }
    /// Current operation cursor, separate from recording length while seeking.
    pub const fn cursor(&self) -> usize {
        self.cursor
    }
    /// Available complete reusable checkpoints.
    pub fn checkpoints(&self) -> &[ReplayCheckpoint] {
        &self.checkpoints
    }

    /// Records already-normalized bound input without altering its payload.
    pub fn push_input(
        &mut self,
        event: GameInputEvent,
        song_time: Timestamp,
    ) -> Result<Vec<JudgeEvent>, ReplayError> {
        let ordinal = u64::try_from(self.records.len()).map_err(|_| ReplayError::Overflow)?;
        self.append_record(ReplayRecord {
            ordinal,
            song_time,
            operation: ReplayOperation::Input(event),
        })
    }
    /// Records exact explicit advance times, including equal-time operations.
    pub fn advance_to(&mut self, song_time: Timestamp) -> Result<Vec<JudgeEvent>, ReplayError> {
        let ordinal = u64::try_from(self.records.len()).map_err(|_| ReplayError::Overflow)?;
        self.append_record(ReplayRecord {
            ordinal,
            song_time,
            operation: ReplayOperation::Advance,
        })
    }
    fn append_record(&mut self, record: ReplayRecord) -> Result<Vec<JudgeEvent>, ReplayError> {
        if self.cursor != self.records.len() || self.boundary_time.is_some() {
            return Err(ReplayError::FutureExists);
        }
        if u64::try_from(self.records.len()).ok() != Some(record.ordinal) {
            return Err(ReplayError::InvalidOrdinal);
        }
        if self
            .records
            .last()
            .is_some_and(|previous| record.song_time < previous.song_time)
        {
            return Err(ReplayError::NonMonotonicSongTime);
        }
        if let ReplayOperation::Input(event) = &record.operation {
            if event.physical.meta().clock_domain != self.header.normalized_clock {
                return Err(ReplayError::ClockDomainMismatch);
            }
        }
        let output = apply(&mut self.engine, &record)?;
        self.results.extend_from_slice(&output);
        self.records.push(record);
        self.cursor += 1;
        Ok(output)
    }
    /// Captures real state and complete result prefix at the current cursor.
    pub fn checkpoint(&mut self) -> Result<(), ReplayError> {
        let checkpoint = ReplayCheckpoint {
            cursor: self.cursor,
            results: self.results.clone(),
            state: self.engine.snapshot()?,
            boundary_time: self.boundary_time,
        };
        if let Some(index) = self.checkpoints.iter().position(|existing| {
            existing.cursor == checkpoint.cursor
                && existing.boundary_time == checkpoint.boundary_time
        }) {
            self.checkpoints[index] = checkpoint;
        } else {
            self.checkpoints.push(checkpoint);
            self.checkpoints.sort_by_key(|checkpoint| checkpoint.cursor);
        }
        Ok(())
    }
    /// Atomically reconstructs forward through recorded operations at/before time.
    /// Repeated decreasing targets provide reverse inspection without reversing
    /// the judge's state machine. Recorded timeouts retain their timestamps;
    /// new boundary misses are stamped at the requested target.
    pub fn seek(&mut self, song_time: Timestamp) -> Result<(), ReplayError> {
        let cursor = self
            .records
            .partition_point(|record| record.song_time <= song_time);
        self.reconstruct(cursor, Some(song_time))
    }
    /// Reconstructs through an exact operation boundary, useful for equal times.
    pub fn seek_cursor(&mut self, cursor: usize) -> Result<(), ReplayError> {
        self.reconstruct(cursor, None)
    }
    fn reconstruct(&mut self, cursor: usize, target: Option<Timestamp>) -> Result<(), ReplayError> {
        if cursor > self.records.len() {
            return Err(ReplayError::InvalidOrdinal);
        }
        let checkpoint = self
            .checkpoints
            .iter()
            .rev()
            .find(|checkpoint| {
                checkpoint.cursor <= cursor
                    && (checkpoint.boundary_time.is_none()
                        || (checkpoint.cursor == cursor && checkpoint.boundary_time == target))
            })
            .expect("origin checkpoint is retained");
        let mut engine = JudgeEngine::from_snapshot(&checkpoint.state)?;
        let mut results = checkpoint.results.clone();
        for record in &self.records[checkpoint.cursor..cursor] {
            results.extend(apply(&mut engine, record)?);
        }
        let boundary_time = target.filter(|target| {
            self.records
                .get(cursor.wrapping_sub(1))
                .is_none_or(|record| *target > record.song_time)
        });
        if let Some(time) = boundary_time {
            if checkpoint.boundary_time != Some(time) {
                results.extend(engine.advance_to(time)?);
            }
        }
        self.engine = engine;
        self.results = results;
        self.cursor = cursor;
        self.boundary_time = boundary_time;
        Ok(())
    }
    /// Explicitly discards recorded future operations/checkpoints after a seek.
    pub fn fork_at_cursor(&mut self) {
        self.records.truncate(self.cursor);
        self.checkpoints.retain(|checkpoint| {
            checkpoint.cursor <= self.cursor && checkpoint.boundary_time.is_none()
        });
        // Materialize the already-applied boundary advance in the new branch,
        // preserving its exact results without invoking the judge twice.
        if let Some(song_time) = self.boundary_time.take() {
            self.records.push(ReplayRecord {
                ordinal: self.cursor as u64,
                song_time,
                operation: ReplayOperation::Advance,
            });
            self.cursor += 1;
        }
    }
    /// Stable complete logical playback hash, excluding wall-clock telemetry.
    pub fn stable_hash(&self) -> Result<u64, ReplayError> {
        let mut bytes = Encoder::new(b"beatkernel-replay-state/v1");
        bytes.u32(self.header.version);
        bytes.bytes(&self.header.chart_identity);
        bytes.bytes(&self.header.rules_identity);
        bytes.bytes(&self.header.options);
        bytes.u64(self.header.seed);
        bytes.u32(self.header.normalized_clock.0);
        bytes.u64(self.cursor as u64);
        bytes.option(self.boundary_time, |out, time| out.i64(time.as_nanos()));
        bytes.bytes(&self.engine.canonical_state_bytes()?);
        bytes.u64(self.results.len() as u64);
        for result in &self.results {
            bytes.result(*result);
        }
        bytes.u64(self.cursor as u64);
        for record in &self.records[..self.cursor] {
            bytes.u64(record.ordinal);
            bytes.i64(record.song_time.as_nanos());
            match &record.operation {
                ReplayOperation::Input(event) => {
                    bytes.u8(0);
                    bytes.input(event);
                }
                ReplayOperation::Advance => bytes.u8(1),
            }
        }
        Ok(hash(&bytes.finish()))
    }
}
fn apply(engine: &mut JudgeEngine, record: &ReplayRecord) -> Result<Vec<JudgeEvent>, JudgeError> {
    match &record.operation {
        ReplayOperation::Input(event) => engine.push_input(event, record.song_time),
        ReplayOperation::Advance => engine.advance_to(record.song_time),
    }
}
