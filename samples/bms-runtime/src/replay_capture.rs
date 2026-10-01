//! Bounded control-thread recording of the actual runtime's accepted operations.

use beatkernel::{
    input::encode_event,
    judge::JudgeEngine,
    replay::{
        REPLAY_VERSION, ReplayError, ReplayHeader, ReplayRecord, ReplayRecorder,
        codec::{ReplayCodecError, ReplayCodecLimits, ReplayFile, encode_replay},
    },
    runtime::RuntimeReport,
    time::{ClockDomainId, Timestamp},
};
use std::{fs::OpenOptions, io::Write, path::Path};

/// Capture, serialization or exclusive output creation failure.
#[derive(Debug)]
pub enum CaptureError {
    /// Practice start must be a nonnegative original-song timestamp.
    InvalidStart,
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
            Self::InvalidStart => write!(f, "BMS replay practice start must be nonnegative"),
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
        Self::new_at(judge, domain, limits, Timestamp::ZERO)
    }

    /// Captures the pristine section judge with its original-song start.
    /// Zero retains the exact v1 profile bytes; positive starts use v2.
    pub fn new_at(
        judge: &JudgeEngine,
        domain: ClockDomainId,
        limits: ReplayCodecLimits,
        start: Timestamp,
    ) -> Result<Self, CaptureError> {
        if start.as_nanos() < 0 {
            return Err(CaptureError::InvalidStart);
        }
        if judge.effective_song_time().is_some() {
            return Err(ReplayError::AlreadyStarted.into());
        }
        let hash = judge.stable_hash().map_err(ReplayError::from)?;
        let mut identity = b"bms-judge-setup/v1:".to_vec();
        identity.extend_from_slice(&hash.to_le_bytes());
        let profile = judge.profile();
        let (prefix, start_bytes): (&[u8], usize) = if start == Timestamp::ZERO {
            (b"bms-judge-profile/v1:", 0)
        } else {
            (b"bms-judge-profile/v2:", 8)
        };
        let options_size = profile
            .windows()
            .len()
            .checked_mul(20)
            .and_then(|bytes| bytes.checked_add(prefix.len() + start_bytes + 16))
            .ok_or(ReplayCodecError::LengthOverflow)?;
        let header_size = options_size
            .checked_add(identity.len())
            .and_then(|bytes| bytes.checked_add(b"beatkernel-bms/builtin-judge/v1".len()))
            .and_then(|bytes| bytes.checked_add(env!("CARGO_PKG_VERSION").len()))
            .ok_or(ReplayCodecError::LengthOverflow)?;
        if header_size > limits.max_header_bytes() {
            return Err(ReplayCodecError::HeaderTooLarge.into());
        }
        let mut options = prefix.to_vec();
        options
            .try_reserve_exact(options_size - options.len())
            .map_err(|_| ReplayCodecError::AllocationFailed)?;
        if start != Timestamp::ZERO {
            options.extend_from_slice(&start.as_nanos().to_le_bytes());
        }
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

#[cfg(test)]
mod section_fixtures {
    use super::*;
    use beatkernel::{
        input::codec::CodecLimits,
        judge::{JudgeGrade, JudgeProfile, JudgeWindow},
        time::Duration,
    };
    use beatkernel_bms::{ParseOptions, parse};

    fn judge() -> JudgeEngine {
        let source = parse("#BPM 120\n#00011:01\n#00112:01\n", ParseOptions::default()).unwrap();
        JudgeEngine::new(
            source.compile().unwrap().chart,
            source.rules(),
            JudgeProfile::new(
                vec![JudgeWindow {
                    grade: JudgeGrade(7),
                    early: Duration::from_nanos(11),
                    late: Duration::from_nanos(23),
                }],
                Duration::from_nanos(-19),
            )
            .unwrap(),
        )
        .unwrap()
    }
    fn limits(header: usize) -> ReplayCodecLimits {
        ReplayCodecLimits::new(8192, 8, header, CodecLimits::new(4096, 1024).unwrap()).unwrap()
    }

    #[test]
    fn zero_is_literal_v1_and_positive_start_prefixes_the_same_body() {
        let judge = judge();
        let domain = ClockDomainId(17);
        let legacy = LiveReplayCapture::new(&judge, domain, limits(4096)).unwrap();
        let zero =
            LiveReplayCapture::new_at(&judge, domain, limits(4096), Timestamp::ZERO).unwrap();
        let mut expected = b"bms-judge-profile/v1:".to_vec();
        expected.extend_from_slice(&(-19_i64).to_le_bytes());
        expected.extend_from_slice(&1_u64.to_le_bytes());
        expected.extend_from_slice(&7_u32.to_le_bytes());
        expected.extend_from_slice(&11_i64.to_le_bytes());
        expected.extend_from_slice(&23_i64.to_le_bytes());
        assert_eq!(legacy.header().options, expected);
        assert_eq!(legacy.header(), zero.header());
        assert_eq!(
            encode_replay(&legacy.into_file(), limits(4096)).unwrap(),
            encode_replay(&zero.into_file(), limits(4096)).unwrap()
        );
        let start = Timestamp::from_nanos(i64::MAX);
        let section = LiveReplayCapture::new_at(&judge, domain, limits(4096), start).unwrap();
        let mut v2 = b"bms-judge-profile/v2:".to_vec();
        v2.extend_from_slice(&i64::MAX.to_le_bytes());
        v2.extend_from_slice(&expected[b"bms-judge-profile/v1:".len()..]);
        assert_eq!(section.header().options, v2);
        let (decoded, decoded_start) = crate::replay_playback::decode_setup(&v2).unwrap();
        assert_eq!(&decoded, judge.profile());
        assert_eq!(decoded_start, start);
        assert!(matches!(
            LiveReplayCapture::new_at(&judge, domain, limits(4096), Timestamp::from_nanos(-1)),
            Err(CaptureError::InvalidStart)
        ));
    }

    #[test]
    fn section_start_bytes_count_toward_the_header_cap() {
        let judge = judge();
        let legacy = LiveReplayCapture::new(&judge, ClockDomainId(17), limits(4096)).unwrap();
        let header = legacy.header();
        let cap = header.chart_identity.len()
            + header.rules_identity.len()
            + header.options.len()
            + env!("CARGO_PKG_VERSION").len();
        assert!(LiveReplayCapture::new(&judge, ClockDomainId(17), limits(cap)).is_ok());
        assert!(matches!(
            LiveReplayCapture::new_at(
                &judge,
                ClockDomainId(17),
                limits(cap + 7),
                Timestamp::from_nanos(1)
            ),
            Err(CaptureError::Codec(ReplayCodecError::HeaderTooLarge))
        ));
        assert!(
            LiveReplayCapture::new_at(
                &judge,
                ClockDomainId(17),
                limits(cap + 8),
                Timestamp::from_nanos(1)
            )
            .is_ok()
        );
    }
}
