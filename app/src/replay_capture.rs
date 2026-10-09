//! Bounded control-thread recording of the actual runtime's accepted operations.

use crate::input_sounds::InputSoundIdentity;
use beatkernel::{
    input::encode_event,
    judge::JudgeEngine,
    replay::{
        codec::{encode_replay, ReplayCodecError, ReplayCodecLimits, ReplayFile},
        ReplayError, ReplayHeader, ReplayRecord, ReplayRecorder, REPLAY_VERSION,
    },
    runtime::RuntimeReport,
    time::{ClockDomainId, Timestamp},
};
use beatkernel_bms::BmsInputMode;
use std::{fs::OpenOptions, io::Write, path::Path};

/// Capture, serialization or exclusive output creation failure.
#[derive(Debug)]
pub enum CaptureError {
    TimingPolicy(crate::replay_timing_policy::PolicyError),
    JudgmentPolicy(crate::replay_judgment_policy::PolicyError),
    InvalidPolicy(&'static str),
    GaugePolicy(crate::replay_gauge_policy::PolicyError),
    /// Practice start must be a nonnegative original-song timestamp.
    InvalidStart,
    /// A finite practice end must be strictly later than its start.
    InvalidEnd,
    /// An accepted operation lies beyond the immutable section boundary.
    OutsideSection,
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
            Self::TimingPolicy(error) => write!(f, "{error}"),
            Self::JudgmentPolicy(error) => write!(f, "{error}"),
            Self::InvalidPolicy(error) => write!(f, "BMS replay policy: {error}"),
            Self::GaugePolicy(error) => write!(f, "{error}"),
            Self::InvalidStart => write!(f, "BMS replay practice start must be nonnegative"),
            Self::InvalidEnd => write!(f, "BMS replay practice end must be later than its start"),
            Self::OutsideSection => {
                write!(f, "BMS replay operation lies outside its finite section")
            }
            Self::Replay(error) => write!(f, "BMS replay capture: {error}"),
            Self::Codec(error) => write!(f, "BMS replay capture: {error}"),
            Self::Io(error) => write!(f, "BMS replay output: {error}"),
        }
    }
}
impl std::error::Error for CaptureError {}

/// Canonical pristine compiled setup, shared by capture and multiplayer identity.
/// The identity excludes asset/device/binding provenance and is noncryptographic.
/// Callers retain canonical codec file-limit validation for their final envelope.
pub fn setup_header(
    judge: &JudgeEngine,
    domain: ClockDomainId,
    limits: ReplayCodecLimits,
    start: Timestamp,
    chart_seed: u64,
) -> Result<ReplayHeader, CaptureError> {
    setup_section_header(judge, domain, limits, start, chart_seed, None)
}

/// Canonical pristine setup with an optional exclusive original-song input end.
/// Finite sections use v4; unlimited sections retain their exact legacy bytes.
pub fn setup_section_header(
    judge: &JudgeEngine,
    domain: ClockDomainId,
    limits: ReplayCodecLimits,
    start: Timestamp,
    chart_seed: u64,
    end: Option<Timestamp>,
) -> Result<ReplayHeader, CaptureError> {
    setup_input_header(
        judge,
        domain,
        limits,
        start,
        chart_seed,
        end,
        BmsInputMode::ButtonOnly,
    )
}

/// Canonical setup retaining the selected input rules as well as section bounds.
/// Button-only modes preserve v1–v4; contact rules use an explicit v5 mode tag.
pub fn setup_input_header(
    judge: &JudgeEngine,
    domain: ClockDomainId,
    limits: ReplayCodecLimits,
    start: Timestamp,
    chart_seed: u64,
    end: Option<Timestamp>,
    input_mode: BmsInputMode,
) -> Result<ReplayHeader, CaptureError> {
    setup_input_sound_header(
        judge, domain, limits, start, chart_seed, end, input_mode, None,
    )
}

/// Canonical complete application setup, retaining nondefault gauge policy.
#[allow(clippy::too_many_arguments)]
pub fn setup_gauge_header(
    judge: &JudgeEngine,
    domain: ClockDomainId,
    limits: ReplayCodecLimits,
    start: Timestamp,
    chart_seed: u64,
    end: Option<Timestamp>,
    input_mode: BmsInputMode,
    input_sounds: Option<InputSoundIdentity>,
    gauge: &crate::gauge::GaugeProfile,
) -> Result<ReplayHeader, CaptureError> {
    let header = setup_input_sound_header(
        judge,
        domain,
        limits,
        start,
        chart_seed,
        end,
        input_mode,
        input_sounds,
    )?;
    crate::replay_gauge_policy::wrap_header(header, gauge, limits)
        .map_err(CaptureError::GaugePolicy)
}

/// Opt-in complete class identity; gauge-only capture stays byte-compatible.
#[allow(clippy::too_many_arguments)]
pub fn setup_play_policy_header(
    judge: &JudgeEngine,
    domain: ClockDomainId,
    limits: ReplayCodecLimits,
    start: Timestamp,
    chart_seed: u64,
    end: Option<Timestamp>,
    input_mode: BmsInputMode,
    input_sounds: Option<InputSoundIdentity>,
    policy: &crate::play_policy::ResolvedPlayPolicy,
) -> Result<ReplayHeader, CaptureError> {
    if judge.profile() != policy.judge() {
        return Err(CaptureError::InvalidPolicy("judge profile differs"));
    }
    policy
        .validate_timing(judge, input_mode)
        .map_err(|_| CaptureError::InvalidPolicy("selected stage configuration differs"))?;
    let header = setup_gauge_header(
        judge,
        domain,
        limits,
        start,
        chart_seed,
        end,
        input_mode,
        input_sounds,
        policy.gauge(),
    )?;
    let header = crate::replay_judgment_policy::wrap_header(header, policy.judgments(), limits)
        .map_err(CaptureError::JudgmentPolicy)?;
    wrap_timing_header(header, policy.timing(), input_mode, limits)
}

/// The staged numerical policy uses its own explicit interaction schema.
pub(crate) fn timing_rules_identity(input_mode: BmsInputMode) -> &'static [u8] {
    match input_mode {
        BmsInputMode::ButtonOnly => b"beatkernel-bms/profiled-builtin-judge/v1",
        BmsInputMode::ButtonOrContact => b"beatkernel-bms/profiled-press-judge/v1",
    }
}

/// Applies the outer recorded timing policy and matching staged rule identity.
/// Absent timing returns the original header unchanged.
pub(crate) fn wrap_timing_header(
    mut header: ReplayHeader,
    timing: Option<&crate::play_policy::ResolvedTimingPolicy>,
    input_mode: BmsInputMode,
    limits: ReplayCodecLimits,
) -> Result<ReplayHeader, CaptureError> {
    if timing.is_some() {
        let identity = timing_rules_identity(input_mode);
        header.rules_identity.clear();
        header
            .rules_identity
            .try_reserve_exact(identity.len())
            .map_err(|_| CaptureError::Codec(ReplayCodecError::AllocationFailed))?;
        header.rules_identity.extend_from_slice(identity);
    }
    crate::replay_timing_policy::wrap_header(header, timing, limits)
        .map_err(CaptureError::TimingPolicy)
}

/// Canonical setup with an optional validated invisible input-sound identity.
/// None preserves legacy bytes; Some extends only the chart identity to v2.
#[allow(clippy::too_many_arguments)]
pub fn setup_input_sound_header(
    judge: &JudgeEngine,
    domain: ClockDomainId,
    limits: ReplayCodecLimits,
    start: Timestamp,
    chart_seed: u64,
    end: Option<Timestamp>,
    input_mode: BmsInputMode,
    input_sounds: Option<InputSoundIdentity>,
) -> Result<ReplayHeader, CaptureError> {
    if start.as_nanos() < 0 {
        return Err(CaptureError::InvalidStart);
    }
    if end.is_some_and(|end| end <= start) {
        return Err(CaptureError::InvalidEnd);
    }
    if judge.effective_song_time().is_some() {
        return Err(ReplayError::AlreadyStarted.into());
    }
    let profile = judge.profile();
    let contact = input_mode == BmsInputMode::ButtonOrContact;
    let rules_identity: &[u8] = if contact {
        b"beatkernel-bms/press-judge/v1"
    } else {
        b"beatkernel-bms/builtin-judge/v1"
    };
    let (prefix, start_bytes): (&[u8], usize) = if contact {
        (
            b"bms-judge-profile/v5:",
            18 + usize::from(end.is_some()) * 8,
        )
    } else if end.is_some() {
        (b"bms-judge-profile/v4:", 24)
    } else if chart_seed != 0 {
        (b"bms-judge-profile/v3:", 16)
    } else if start == Timestamp::ZERO {
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
    let (identity_prefix, identity_size): (&[u8], usize) = if input_sounds.is_some() {
        (b"bms-judge-setup/v2:", b"bms-judge-setup/v2:".len() + 16)
    } else {
        (b"bms-judge-setup/v1:", b"bms-judge-setup/v1:".len() + 8)
    };
    let header_size = options_size
        .checked_add(identity_size)
        .and_then(|bytes| bytes.checked_add(rules_identity.len()))
        .and_then(|bytes| bytes.checked_add(env!("CARGO_PKG_VERSION").len()))
        .ok_or(ReplayCodecError::LengthOverflow)?;
    if header_size > limits.max_header_bytes() {
        return Err(ReplayCodecError::HeaderTooLarge.into());
    }
    let hash = judge.stable_hash().map_err(ReplayError::from)?;
    let mut identity = Vec::new();
    identity
        .try_reserve_exact(identity_size)
        .map_err(|_| ReplayCodecError::AllocationFailed)?;
    identity.extend_from_slice(identity_prefix);
    identity.extend_from_slice(&hash.to_le_bytes());
    if let Some(input_sounds) = input_sounds {
        identity.extend_from_slice(&input_sounds.fingerprint().to_le_bytes());
    }
    let mut rules = Vec::new();
    rules
        .try_reserve_exact(rules_identity.len())
        .map_err(|_| ReplayCodecError::AllocationFailed)?;
    rules.extend_from_slice(rules_identity);
    let mut options = Vec::new();
    options
        .try_reserve_exact(options_size)
        .map_err(|_| ReplayCodecError::AllocationFailed)?;
    options.extend_from_slice(prefix);
    if contact {
        options.push(1);
    }
    if contact || end.is_some() || chart_seed != 0 {
        options.extend_from_slice(&chart_seed.to_le_bytes());
    }
    if contact || end.is_some() || chart_seed != 0 || start != Timestamp::ZERO {
        options.extend_from_slice(&start.as_nanos().to_le_bytes());
    }
    if contact {
        options.push(u8::from(end.is_some()));
    }
    if let Some(end) = end {
        options.extend_from_slice(&end.as_nanos().to_le_bytes());
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
    Ok(ReplayHeader {
        version: REPLAY_VERSION,
        chart_identity: identity,
        rules_identity: rules,
        options,
        seed: 0,
        normalized_clock: domain,
    })
}

/// Optional application-owned capture; never judges or accesses native clocks.
///
/// The encoded-byte cap is an exact durable-data cap, not a process-memory cap.
/// Per-report candidate encoding allocates on the caller's control thread.
pub struct LiveReplayCapture {
    recorder: ReplayRecorder,
    limits: ReplayCodecLimits,
    header_bytes: usize,
    encoded_bytes: usize,
    end: Option<Timestamp>,
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
        Self::new_at_with_chart_seed(judge, domain, limits, start, 0)
    }

    /// Captures explicit BMS branch provenance separately from the rule seed.
    /// Zero preserves v1/v2 bytes; nonzero uses v3 with seed then section start.
    pub fn new_at_with_chart_seed(
        judge: &JudgeEngine,
        domain: ClockDomainId,
        limits: ReplayCodecLimits,
        start: Timestamp,
        chart_seed: u64,
    ) -> Result<Self, CaptureError> {
        Self::new_section(judge, domain, limits, start, chart_seed, None)
    }

    /// Capture accepted operations through a fixed section end without adding
    /// a final advance. Bound input excludes the end; an advance may equal it.
    pub fn new_section(
        judge: &JudgeEngine,
        domain: ClockDomainId,
        limits: ReplayCodecLimits,
        start: Timestamp,
        chart_seed: u64,
        end: Option<Timestamp>,
    ) -> Result<Self, CaptureError> {
        Self::new_with_input_mode(
            judge,
            domain,
            limits,
            start,
            chart_seed,
            end,
            BmsInputMode::ButtonOnly,
        )
    }

    /// Capture the pristine judge with an explicit input mode and section bounds.
    pub fn new_with_input_mode(
        judge: &JudgeEngine,
        domain: ClockDomainId,
        limits: ReplayCodecLimits,
        start: Timestamp,
        chart_seed: u64,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
    ) -> Result<Self, CaptureError> {
        Self::new_with_input_sounds(
            judge, domain, limits, start, chart_seed, end, input_mode, None,
        )
    }

    /// Captures the same accepted operations with optional invisible sound
    /// compatibility metadata. Final header/file budgets include the extension.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_input_sounds(
        judge: &JudgeEngine,
        domain: ClockDomainId,
        limits: ReplayCodecLimits,
        start: Timestamp,
        chart_seed: u64,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        input_sounds: Option<InputSoundIdentity>,
    ) -> Result<Self, CaptureError> {
        Self::new_with_gauge(
            judge,
            domain,
            limits,
            start,
            chart_seed,
            end,
            input_mode,
            input_sounds,
            &crate::gauge::GaugeProfile::default(),
        )
    }

    /// Records a resolved gauge policy in the immutable setup identity.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_gauge(
        judge: &JudgeEngine,
        domain: ClockDomainId,
        limits: ReplayCodecLimits,
        start: Timestamp,
        chart_seed: u64,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        input_sounds: Option<InputSoundIdentity>,
        gauge: &crate::gauge::GaugeProfile,
    ) -> Result<Self, CaptureError> {
        let header = setup_gauge_header(
            judge,
            domain,
            limits,
            start,
            chart_seed,
            end,
            input_mode,
            input_sounds,
            gauge,
        )?;
        let header_bytes =
            encode_replay(&ReplayFile::new(header.clone(), Vec::new()), limits)?.len();
        Ok(Self {
            recorder: ReplayRecorder::new(header)?,
            limits,
            header_bytes,
            encoded_bytes: header_bytes,
            end,
        })
    }

    /// Records explicit grade meaning together with judge, gauge and section.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_policy(
        judge: &JudgeEngine,
        domain: ClockDomainId,
        limits: ReplayCodecLimits,
        start: Timestamp,
        chart_seed: u64,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        input_sounds: Option<InputSoundIdentity>,
        policy: &crate::play_policy::ResolvedPlayPolicy,
    ) -> Result<Self, CaptureError> {
        let header = setup_play_policy_header(
            judge,
            domain,
            limits,
            start,
            chart_seed,
            end,
            input_mode,
            input_sounds,
            policy,
        )?;
        let header_bytes =
            encode_replay(&ReplayFile::new(header.clone(), Vec::new()), limits)?.len();
        Ok(Self {
            recorder: ReplayRecorder::new(header)?,
            limits,
            header_bytes,
            encoded_bytes: header_bytes,
            end,
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
        if self.end.is_some_and(|end| {
            report.song_time > end || (report.song_time == end && !report.bound_inputs.is_empty())
        }) {
            return Err(CaptureError::OutsideSection);
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
    /// Encodes the accepted prefix with its original limits, without file I/O.
    /// Consuming the capture makes this one export attempt; no judge is rerun.
    pub fn into_bytes(self) -> Result<Vec<u8>, CaptureError> {
        let limits = self.limits;
        Ok(encode_replay(&self.into_file(), limits)?)
    }
    /// Encodes before exclusive file creation; invoke after native cleanup.
    ///
    /// Existing files remain untouched. A write/flush failure can leave a partial
    /// newly created file. Success means a flushed write, not power-loss safety.
    pub fn save_new(self, path: &Path) -> Result<usize, CaptureError> {
        let bytes = self.into_bytes()?;
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
    use beatkernel_bms::{parse, ParseOptions};

    fn judge() -> JudgeEngine {
        let source = parse(
            "#BPM 120\n#WAV01 head.wav\n#00011:01\n#00112:01\n",
            ParseOptions::default(),
        )
        .unwrap();
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
    fn chart_seed_zero_preserves_legacy_and_nonzero_has_literal_v3_extent() {
        let judge = judge();
        let domain = ClockDomainId(17);
        for start in [Timestamp::ZERO, Timestamp::from_nanos(i64::MAX)] {
            let old = LiveReplayCapture::new_at(&judge, domain, limits(4096), start).unwrap();
            let zero =
                LiveReplayCapture::new_at_with_chart_seed(&judge, domain, limits(4096), start, 0)
                    .unwrap();
            assert_eq!(
                encode_replay(&old.into_file(), limits(4096)).unwrap(),
                encode_replay(&zero.into_file(), limits(4096)).unwrap()
            );
            for seed in [1, u64::MAX] {
                let capture = LiveReplayCapture::new_at_with_chart_seed(
                    &judge,
                    domain,
                    limits(4096),
                    start,
                    seed,
                )
                .unwrap();
                let mut literal = b"bms-judge-profile/v3:".to_vec();
                literal.extend_from_slice(&seed.to_le_bytes());
                literal.extend_from_slice(&start.as_nanos().to_le_bytes());
                literal.extend_from_slice(&(-19_i64).to_le_bytes());
                literal.extend_from_slice(&1_u64.to_le_bytes());
                literal.extend_from_slice(&7_u32.to_le_bytes());
                literal.extend_from_slice(&11_i64.to_le_bytes());
                literal.extend_from_slice(&23_i64.to_le_bytes());
                assert_eq!(capture.header().options, literal);
                assert_eq!(capture.header().seed, 0);
                let (profile, decoded_start, decoded_seed) =
                    crate::replay_playback::decode_chart_setup(&literal).unwrap();
                assert_eq!(&profile, judge.profile());
                assert_eq!(decoded_start, start);
                assert_eq!(decoded_seed, seed);
                let header = capture.header();
                let cap = header.chart_identity.len()
                    + header.rules_identity.len()
                    + header.options.len()
                    + env!("CARGO_PKG_VERSION").len();
                assert!(LiveReplayCapture::new_at_with_chart_seed(
                    &judge,
                    domain,
                    limits(cap),
                    start,
                    seed
                )
                .is_ok());
                assert!(matches!(
                    LiveReplayCapture::new_at_with_chart_seed(
                        &judge,
                        domain,
                        limits(cap - 1),
                        start,
                        seed
                    ),
                    Err(CaptureError::Codec(ReplayCodecError::HeaderTooLarge))
                ));
            }
        }
        assert!(matches!(
            LiveReplayCapture::new_at_with_chart_seed(
                &judge,
                domain,
                limits(4096),
                Timestamp::from_nanos(-1),
                1
            ),
            Err(CaptureError::InvalidStart)
        ));
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
        assert!(LiveReplayCapture::new_at(
            &judge,
            ClockDomainId(17),
            limits(cap + 8),
            Timestamp::from_nanos(1)
        )
        .is_ok());
    }
}
