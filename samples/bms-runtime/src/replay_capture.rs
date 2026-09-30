//! Bounded control-thread recording of the actual runtime's accepted operations.

use beatkernel::{
    input::encode_event,
    judge::JudgeEngine,
    replay::{
        codec::{encode_replay, ReplayCodecError, ReplayCodecLimits, ReplayFile},
        ReplayError, ReplayHeader, ReplayRecord, ReplayRecorder, REPLAY_VERSION,
    },
    runtime::RuntimeReport,
    time::ClockDomainId,
};
use std::{fs::OpenOptions, io::Write, path::Path};

/// Capture, serialization or exclusive output creation failure.
#[derive(Debug)]
pub enum CaptureError {
    /// The existing logical replay validator rejected the operation/setup.
    Replay(ReplayError),
    /// The existing bounded durable codec rejected data or capacity.
    Codec(ReplayCodecError),
    /// A newly created output could not be written, or the path already exists.
    Io(std::io::Error),
}
impl From<ReplayError> for CaptureError {
    fn from(error: ReplayError) -> Self {
        Self::Replay(error)
    }
}
impl From<ReplayCodecError> for CaptureError {
    fn from(error: ReplayCodecError) -> Self {
        Self::Codec(error)
    }
}
impl From<std::io::Error> for CaptureError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Replay(error) => write!(f, "BMS replay capture: {error}"),
            Self::Codec(error) => write!(f, "BMS replay capture: {error}"),
            Self::Io(error) => write!(f, "BMS replay output: {error}"),
        }
    }
}
impl std::error::Error for CaptureError {}

/// Optional application-owned capture; never judges or accesses native clocks.
///
/// The encoded-byte cap is an exact durable-data cap, not a process-memory cap.
/// Per-report candidate encoding allocates on the caller's control thread.
pub struct LiveReplayCapture {
    recorder: ReplayRecorder,
    limits: ReplayCodecLimits,
    header_bytes: usize,
    encoded_bytes: usize,
}
impl LiveReplayCapture {
    /// Fingerprints a pristine compiled judge setup, including its profile.
    ///
    /// This application identity is noncryptographic and excludes source-file,
    /// audio-asset and device identity. Consumers must reconstruct and compare
    /// the same setup before replay. Complete snapshot support is required.
    pub fn new(
        judge: &JudgeEngine,
        domain: ClockDomainId,
        limits: ReplayCodecLimits,
    ) -> Result<Self, CaptureError> {
        if judge.effective_song_time().is_some() {
            return Err(ReplayError::AlreadyStarted.into());
        }
        let hash = judge.stable_hash().map_err(ReplayError::from)?;
        let mut identity = b"bms-judge-setup/v1:".to_vec();
        identity.extend_from_slice(&hash.to_le_bytes());
        let profile = judge.profile();
        let options_size = profile
            .windows()
            .len()
            .checked_mul(20)
            .and_then(|bytes| bytes.checked_add(b"bms-judge-profile/v1:".len() + 16))
            .ok_or(ReplayCodecError::LengthOverflow)?;
        let header_size = options_size
            .checked_add(identity.len())
            .and_then(|bytes| bytes.checked_add(b"beatkernel-bms/builtin-judge/v1".len()))
            .and_then(|bytes| bytes.checked_add(env!("CARGO_PKG_VERSION").len()))
            .ok_or(ReplayCodecError::LengthOverflow)?;
        if header_size > limits.max_header_bytes() {
            return Err(ReplayCodecError::HeaderTooLarge.into());
        }
        let mut options = b"bms-judge-profile/v1:".to_vec();
        options
            .try_reserve_exact(options_size - options.len())
            .map_err(|_| ReplayCodecError::AllocationFailed)?;
        options.extend_from_slice(&profile.input_offset().as_nanos().to_le_bytes());
        options.extend_from_slice(
            &u64::try_from(profile.windows().len())
                .map_err(|_| ReplayCodecError::LengthOverflow)?
                .to_le_bytes(),
        );
        for window in profile.windows() {
            options.extend_from_slice(&window.grade.0.to_le_bytes());
            options.extend_from_slice(&window.early.as_nanos().to_le_bytes());
            options.extend_from_slice(&window.late.as_nanos().to_le_bytes());
        }
        let header = ReplayHeader {
            version: REPLAY_VERSION,
            chart_identity: identity,
            rules_identity: b"beatkernel-bms/builtin-judge/v1".to_vec(),
            options,
            seed: 0,
            normalized_clock: domain,
        };
        let header_bytes =
            encode_replay(&ReplayFile::new(header.clone(), Vec::new()), limits)?.len();
        Ok(Self {
            recorder: ReplayRecorder::new(header)?,
            limits,
            header_bytes,
            encoded_bytes: header_bytes,
        })
    }

    /// Records one report in call order, atomically with respect to validation.
    ///
    /// Successful input prefixes survive a judge error. A report rejected by
    /// capture leaves the prior log intact even though the live judge may have
    /// already accepted it; callers must stop and label that log as a prefix.
    pub fn record_report(&mut self, report: &RuntimeReport) -> Result<(), CaptureError> {
        let advance = report.input.is_none() && report.judge_error.is_none();
        let count = report
            .bound_inputs
            .len()
            .checked_add(usize::from(advance))
            .ok_or(ReplayCodecError::LengthOverflow)?;
        if count == 0 {
            return Ok(());
        }
        let total = self
            .records()
            .len()
            .checked_add(count)
            .ok_or(ReplayCodecError::LengthOverflow)?;
        if total > self.limits.max_records() {
            return Err(ReplayCodecError::TooManyRecords.into());
        }
        if self
            .records()
            .last()
            .is_some_and(|last| last.song_time > report.song_time)
        {
            return Err(ReplayError::NonMonotonicSongTime.into());
        }
        // Validate every borrowed payload before ReplayRecorder clones it.
        let mut minimum_bytes = self.encoded_bytes;
        for input in &report.bound_inputs {
            if input.physical.meta().clock_domain != self.header().normalized_clock {
                return Err(ReplayError::ClockDomainMismatch.into());
            }
            let encoded = encode_event(&input.physical, self.limits.input_limits())
                .map_err(ReplayCodecError::from)?;
            minimum_bytes = minimum_bytes
                .checked_add(encoded.len())
                .ok_or(ReplayCodecError::LengthOverflow)?;
            if minimum_bytes > self.limits.max_file_bytes() {
                return Err(ReplayCodecError::FileTooLarge.into());
            }
        }
        // Reuse the canonical codec, including tags and embedded event framing,
        // rather than duplicating wire-format sizes. Work scales with this
        // report, not the accumulated session; the envelope size is constant.
        let mut candidate = ReplayRecorder::new(self.header().clone())?;
        candidate.record_report(report)?;
        let (header, records) = candidate.into_parts();
        let candidate_bytes = encode_replay(&ReplayFile::new(header, records), self.limits)?.len();
        let added = candidate_bytes
            .checked_sub(self.header_bytes)
            .ok_or(ReplayCodecError::LengthOverflow)?;
        let encoded_bytes = self
            .encoded_bytes
            .checked_add(added)
            .ok_or(ReplayCodecError::LengthOverflow)?;
        if encoded_bytes > self.limits.max_file_bytes() {
            return Err(ReplayCodecError::FileTooLarge.into());
        }
        self.recorder.record_report(report)?;
        self.encoded_bytes = encoded_bytes;
        Ok(())
    }

    /// Application setup identity and normalized input domain.
    pub fn header(&self) -> &ReplayHeader {
        self.recorder.header()
    }
    /// Ordered accepted operations retained so far.
    pub fn records(&self) -> &[ReplayRecord] {
        self.recorder.records()
    }
    /// Exact byte length of this log in the current canonical file envelope.
    pub const fn encoded_bytes(&self) -> usize {
        self.encoded_bytes
    }
    /// Transfers the accepted log without executing another judge.
    pub fn into_file(self) -> ReplayFile {
        let (header, records) = self.recorder.into_parts();
        ReplayFile::new(header, records)
    }
    /// Encodes before exclusive file creation; invoke after native cleanup.
    ///
    /// Existing files remain untouched. A write/flush failure can leave a partial
    /// newly created file. Success means a flushed write, not power-loss safety.
    pub fn save_new(self, path: &Path) -> Result<usize, CaptureError> {
        let limits = self.limits;
        let bytes = encode_replay(&self.into_file(), limits)?;
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(&bytes)?;
        file.flush()?;
        Ok(bytes.len())
    }
}
