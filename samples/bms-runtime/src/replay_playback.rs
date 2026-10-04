//! Application identity checks and logical reconstruction of captured BMS play.

use crate::replay_capture::{CaptureError, setup_input_header};
#[cfg(test)]
use crate::replay_capture::LiveReplayCapture;
use beatkernel::{
    judge::{JudgeEngine, JudgeError, JudgeGrade, JudgeProfile, JudgeWindow},
    replay::{
        ReplayError, ReplayOperation, ReplaySession,
        codec::{ReplayCodecError, ReplayCodecLimits, ReplayFile, decode_replay, encode_replay},
    },
    time::{Duration, Timestamp},
};
use beatkernel_bms::{BmsChart, BmsError, BmsInputMode};
use std::io::{ErrorKind, Read};

/// Invalid replay metadata, compatibility, bounded data or logical reconstruction.
#[derive(Debug)]
pub enum PlaybackError {
    /// The recorded original-song section could not be selected.
    Section(Box<dyn std::error::Error>),
    /// Invalid versioned profile layout or arithmetic extent.
    Metadata(&'static str),
    /// The application setup/version differs from the recording.
    IdentityMismatch(&'static str),
    /// Canonical file or input validation failed.
    Codec(ReplayCodecError),
    /// A pristine setup fingerprint could not be produced.
    Capture(CaptureError),
    /// The recorded profile or reconstructed judge is invalid.
    Judge(JudgeError),
    /// The supplied BMS chart could not be compiled.
    Bms(BmsError),
    /// An operation or snapshot could not be reconstructed.
    Replay(ReplayError),
    /// Reading the supplied stream failed.
    Io(std::io::Error),
}
impl From<Box<dyn std::error::Error>> for PlaybackError {
    fn from(error: Box<dyn std::error::Error>) -> Self {
        Self::Section(error)
    }
}
impl From<ReplayCodecError> for PlaybackError {
    fn from(error: ReplayCodecError) -> Self {
        Self::Codec(error)
    }
}
impl From<CaptureError> for PlaybackError {
    fn from(error: CaptureError) -> Self {
        Self::Capture(error)
    }
}
impl From<JudgeError> for PlaybackError {
    fn from(error: JudgeError) -> Self {
        Self::Judge(error)
    }
}
impl From<BmsError> for PlaybackError {
    fn from(error: BmsError) -> Self {
        Self::Bms(error)
    }
}
impl From<ReplayError> for PlaybackError {
    fn from(error: ReplayError) -> Self {
        Self::Replay(error)
    }
}
impl From<std::io::Error> for PlaybackError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl std::fmt::Display for PlaybackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BMS logical replay: {self:?}")
    }
}
impl std::error::Error for PlaybackError {}

/// Reads and decodes a bounded stream without trusting a file metadata length.
///
/// An extra byte beyond the cap rejects the complete log. No judge or native
/// device is accessed; interrupted reads are retried, other errors propagate.
pub fn read_replay(
    reader: &mut impl Read,
    limits: ReplayCodecLimits,
) -> Result<ReplayFile, PlaybackError> {
    let mut bytes = Vec::new();
    let mut block = [0u8; 8192];
    loop {
        let count = match reader.read(&mut block) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        };
        let length = bytes
            .len()
            .checked_add(count)
            .ok_or(ReplayCodecError::LengthOverflow)?;
        if length > limits.max_file_bytes() {
            return Err(ReplayCodecError::FileTooLarge.into());
        }
        bytes
            .try_reserve(count)
            .map_err(|_| ReplayCodecError::AllocationFailed)?;
        bytes.extend_from_slice(&block[..count]);
    }
    Ok(decode_replay(&bytes, limits)?)
}

/// Explicit recorded setup retaining section bounds and input interaction mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedSetup {
    pub profile: JudgeProfile,
    pub start: Timestamp,
    pub chart_seed: u64,
    pub end: Option<Timestamp>,
    pub input_mode: BmsInputMode,
}

/// Decodes the exact unlimited versioned profile captured by the BMS runtime.
///
/// Extent checks precede window allocation. The resulting profile applies its
/// signed offset once through JudgeEngine, never by editing recorded times.
pub fn decode_profile(options: &[u8]) -> Result<JudgeProfile, PlaybackError> {
    Ok(decode_setup(options)?.0)
}

/// Decodes the profile and original-song practice start without shifting input times.
/// V1 means zero; v2 requires a positive i64 little-endian start before the v1 body.
pub fn decode_setup(options: &[u8]) -> Result<(JudgeProfile, Timestamp), PlaybackError> {
    let (profile, start, _) = decode_chart_setup(options)?;
    Ok((profile, start))
}

/// Decodes canonical profile, original-song start and BMS branch seed.
/// Legacy v1/v2 imply seed zero; v3 requires a nonzero seed and nonnegative start.
/// V4 and v5 are refused because this tuple cannot preserve endpoints/input mode.
pub fn decode_chart_setup(options: &[u8]) -> Result<(JudgeProfile, Timestamp, u64), PlaybackError> {
    let setup = decode_recorded_setup(options, false)?;
    Ok((setup.profile, setup.start, setup.chart_seed))
}

/// Decodes canonical v1–v5 metadata without dropping section bounds/input mode.
pub fn decode_section_setup(options: &[u8]) -> Result<RecordedSetup, PlaybackError> {
    decode_recorded_setup(options, true)
}

fn decode_recorded_setup(
    options: &[u8],
    allow_extended: bool,
) -> Result<RecordedSetup, PlaybackError> {
    let (bytes, start, chart_seed, end, input_mode) = if let Some(bytes) =
        options.strip_prefix(b"bms-judge-profile/v1:")
    {
        (bytes, Timestamp::ZERO, 0, None, BmsInputMode::ButtonOnly)
    } else if let Some(bytes) = options.strip_prefix(b"bms-judge-profile/v2:") {
        let encoded = bytes
            .get(..8)
            .ok_or(PlaybackError::Metadata("truncated section start"))?;
        let start = i64::from_le_bytes(encoded.try_into().expect("checked section start"));
        if start <= 0 {
            return Err(PlaybackError::Metadata("v2 section start must be positive"));
        }
        (
            &bytes[8..],
            Timestamp::from_nanos(start),
            0,
            None,
            BmsInputMode::ButtonOnly,
        )
    } else if let Some(bytes) = options.strip_prefix(b"bms-judge-profile/v3:") {
        let encoded = bytes
            .get(..16)
            .ok_or(PlaybackError::Metadata("truncated chart seed/start"))?;
        let seed = u64::from_le_bytes(encoded[..8].try_into().expect("checked chart seed"));
        let start = i64::from_le_bytes(encoded[8..].try_into().expect("checked section start"));
        if seed == 0 || start < 0 {
            return Err(PlaybackError::Metadata(
                "v3 requires nonzero chart seed and nonnegative start",
            ));
        }
        (
            &bytes[16..],
            Timestamp::from_nanos(start),
            seed,
            None,
            BmsInputMode::ButtonOnly,
        )
    } else if let Some(bytes) = options.strip_prefix(b"bms-judge-profile/v4:") {
        if !allow_extended {
            return Err(PlaybackError::Metadata(
                "finite replay requires a section-aware consumer",
            ));
        }
        let encoded = bytes
            .get(..24)
            .ok_or(PlaybackError::Metadata("truncated chart seed/start/end"))?;
        let seed = u64::from_le_bytes(encoded[..8].try_into().expect("checked chart seed"));
        let start = i64::from_le_bytes(encoded[8..16].try_into().expect("checked section start"));
        let end = i64::from_le_bytes(encoded[16..].try_into().expect("checked section end"));
        if start < 0 || end <= start {
            return Err(PlaybackError::Metadata(
                "v4 requires nonnegative start and a later end",
            ));
        }
        (
            &bytes[24..],
            Timestamp::from_nanos(start),
            seed,
            Some(Timestamp::from_nanos(end)),
            BmsInputMode::ButtonOnly,
        )
    } else if let Some(bytes) = options.strip_prefix(b"bms-judge-profile/v5:") {
        if !allow_extended {
            return Err(PlaybackError::Metadata(
                "contact replay requires an input-mode-aware consumer",
            ));
        }
        let encoded = bytes.get(..18).ok_or(PlaybackError::Metadata(
            "truncated input mode/seed/start/end tag",
        ))?;
        if encoded[0] != 1 {
            return Err(PlaybackError::Metadata("v5 requires contact input mode 1"));
        }
        let seed = u64::from_le_bytes(encoded[1..9].try_into().expect("checked chart seed"));
        let start = i64::from_le_bytes(encoded[9..17].try_into().expect("checked section start"));
        if start < 0 {
            return Err(PlaybackError::Metadata("v5 requires nonnegative start"));
        }
        let (body, end) = match encoded[17] {
            0 => (&bytes[18..], None),
            1 => {
                let encoded_end = bytes
                    .get(18..26)
                    .ok_or(PlaybackError::Metadata("truncated section end"))?;
                let end = i64::from_le_bytes(encoded_end.try_into().expect("checked section end"));
                if end <= start {
                    return Err(PlaybackError::Metadata("v5 end must be later than start"));
                }
                (&bytes[26..], Some(Timestamp::from_nanos(end)))
            }
            _ => return Err(PlaybackError::Metadata("invalid v5 end tag")),
        };
        (
            body,
            Timestamp::from_nanos(start),
            seed,
            end,
            BmsInputMode::ButtonOrContact,
        )
    } else {
        return Err(PlaybackError::Metadata("unsupported profile schema"));
    };
    let fixed = bytes
        .get(..16)
        .ok_or(PlaybackError::Metadata("truncated profile header"))?;
    let offset = i64::from_le_bytes(fixed[..8].try_into().expect("checked profile offset"));
    let count = u64::from_le_bytes(fixed[8..16].try_into().expect("checked profile count"));
    let count = usize::try_from(count)
        .map_err(|_| PlaybackError::Metadata("unrepresentable profile window count"))?;
    if count == 0 {
        return Err(PlaybackError::Metadata(
            "profile requires at least one window",
        ));
    }
    let length = count
        .checked_mul(20)
        .and_then(|length| length.checked_add(16))
        .ok_or(PlaybackError::Metadata("profile extent overflow"))?;
    if bytes.len() != length {
        return Err(PlaybackError::Metadata(
            "profile extent does not match count",
        ));
    }
    let mut windows = Vec::new();
    windows
        .try_reserve_exact(count)
        .map_err(|_| ReplayCodecError::AllocationFailed)?;
    for window in bytes[16..].chunks_exact(20) {
        windows.push(JudgeWindow {
            grade: JudgeGrade(u32::from_le_bytes(
                window[..4].try_into().expect("checked profile grade"),
            )),
            early: Duration::from_nanos(i64::from_le_bytes(
                window[4..12]
                    .try_into()
                    .expect("checked profile early bound"),
            )),
            late: Duration::from_nanos(i64::from_le_bytes(
                window[12..20]
                    .try_into()
                    .expect("checked profile late bound"),
            )),
        });
    }
    Ok(RecordedSetup {
        profile: JudgeProfile::new(windows, Duration::from_nanos(offset))?,
        start,
        chart_seed,
        end,
        input_mode,
    })
}

/// Checks the entire log and application setup before executing any operation.
///
/// The supplied source recompiles with its actual builtin rules and the recorded
/// profile. Inputs are already bound/normalized, so replay never rebinds them or
/// remaps native clocks. Empty and failed-session prefix logs remain valid; no
/// synthetic final advance is appended. This performs logical reconstruction,
/// without PCM loading, audio commands or native output.
pub fn reconstruct(
    source: &BmsChart,
    file: ReplayFile,
    limits: ReplayCodecLimits,
) -> Result<ReplaySession, PlaybackError> {
    reconstruct_with_section(source, file, limits, false)
}

/// Reconstruct a validated finite or unlimited accepted prefix using the core judge.
/// No operation is synthesized at the configured end or at the end of the log.
pub fn reconstruct_section(
    source: &BmsChart,
    file: ReplayFile,
    limits: ReplayCodecLimits,
) -> Result<ReplaySession, PlaybackError> {
    reconstruct_with_section(source, file, limits, true)
}

fn reconstruct_with_section(
    source: &BmsChart,
    file: ReplayFile,
    limits: ReplayCodecLimits,
    allow_extended: bool,
) -> Result<ReplaySession, PlaybackError> {
    let judge = validate_recorded_setup(source, &file, limits, allow_extended)?;
    Ok(ReplaySession::from_records(
        file.header,
        judge,
        file.records,
    )?)
}

/// Validates the complete bounded log and pristine section setup before operations.
/// Borrows the recording so preparation can reject incompatible data before PCM work.
pub fn validate_setup(
    source: &BmsChart,
    file: &ReplayFile,
    limits: ReplayCodecLimits,
) -> Result<JudgeEngine, PlaybackError> {
    validate_recorded_setup(source, file, limits, false)
}

/// Validate an entire canonical section log and rebuild its pristine judge.
/// Selects original heads from the recorded start; the end bounds operations,
/// without manufacturing misses for unplayed chart objects after that boundary.
pub fn validate_section_setup(
    source: &BmsChart,
    file: &ReplayFile,
    limits: ReplayCodecLimits,
) -> Result<JudgeEngine, PlaybackError> {
    validate_recorded_setup(source, file, limits, true)
}

fn validate_recorded_setup(
    source: &BmsChart,
    file: &ReplayFile,
    limits: ReplayCodecLimits,
    allow_extended: bool,
) -> Result<JudgeEngine, PlaybackError> {
    // Also validates files assembled directly by callers, not only decoded logs.
    encode_replay(file, limits)?;
    if file.runtime_version != env!("CARGO_PKG_VERSION") {
        return Err(PlaybackError::IdentityMismatch("runtime version"));
    }
    if file.header.seed != 0 {
        return Err(PlaybackError::IdentityMismatch("BMS rule seed"));
    }
    let RecordedSetup {
        profile,
        start,
        chart_seed,
        end,
        input_mode,
    } = decode_recorded_setup(&file.header.options, allow_extended)?;
    let rules_identity: &[u8] = match input_mode {
        BmsInputMode::ButtonOnly => b"beatkernel-bms/builtin-judge/v1",
        BmsInputMode::ButtonOrContact => b"beatkernel-bms/press-judge/v1",
    };
    if file.header.rules_identity != rules_identity {
        return Err(PlaybackError::IdentityMismatch("BMS rule schema"));
    }
    if end.is_some_and(|end| {
        file.records.iter().any(|record| {
            record.song_time > end
                || (record.song_time == end
                    && matches!(&record.operation, ReplayOperation::Input(_)))
        })
    }) {
        return Err(PlaybackError::Metadata(
            "recorded operation lies outside its finite section",
        ));
    }
    let selected = crate::section_start::source_at(source, start)?;
    let compiled = selected.compile()?;
    let constructor =
        if input_mode == BmsInputMode::ButtonOrContact && !selected.invisible.is_empty() {
            JudgeEngine::new_with_contacts
        } else {
            JudgeEngine::new
        };
    let judge = constructor(
        compiled.chart,
        selected.rules_with_input_mode(input_mode),
        profile,
    )?;
    let expected = setup_input_header(
        &judge,
        file.header.normalized_clock,
        limits,
        start,
        chart_seed,
        end,
        input_mode,
    )?;
    if expected != file.header {
        return Err(PlaybackError::IdentityMismatch(
            "compiled judge setup/profile",
        ));
    }
    Ok(judge)
}

#[cfg(test)]
mod section_fixtures {
    use super::*;
    use beatkernel::{input::codec::CodecLimits, time::ClockDomainId};
    use beatkernel_bms::{ParseOptions, parse};

    fn limits() -> ReplayCodecLimits {
        ReplayCodecLimits::new(8192, 8, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
    }
    fn profile() -> JudgeProfile {
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(7),
                early: Duration::from_nanos(11),
                late: Duration::from_nanos(23),
            }],
            Duration::from_nanos(-19),
        )
        .unwrap()
    }
    fn file(source: &BmsChart, start: Timestamp) -> ReplayFile {
        let selected = crate::section_start::source_at(source, start).unwrap();
        let judge = JudgeEngine::new(
            selected.compile().unwrap().chart,
            selected.rules(),
            profile(),
        )
        .unwrap();
        LiveReplayCapture::new_at(&judge, ClockDomainId(17), limits(), start)
            .unwrap()
            .into_file()
    }
    fn source() -> BmsChart {
        parse(
            "#BPM 120\n#WAV01 head.wav\n#00011:01\n#00112:01\n",
            ParseOptions::default(),
        )
        .unwrap()
    }

    #[test]
    fn v3_metadata_is_canonical_and_legacy_wrappers_preserve_shapes() {
        let legacy = file(&source(), Timestamp::ZERO).header.options;
        let mut valid = b"bms-judge-profile/v3:".to_vec();
        valid.extend_from_slice(&u64::MAX.to_le_bytes());
        valid.extend_from_slice(&i64::MAX.to_le_bytes());
        valid.extend_from_slice(&legacy[b"bms-judge-profile/v1:".len()..]);
        assert_eq!(
            decode_chart_setup(&valid).unwrap(),
            (profile(), Timestamp::from_nanos(i64::MAX), u64::MAX)
        );
        assert_eq!(
            decode_setup(&valid).unwrap(),
            (profile(), Timestamp::from_nanos(i64::MAX))
        );
        assert_eq!(decode_profile(&valid).unwrap(), profile());
        assert_eq!(decode_chart_setup(&legacy).unwrap().2, 0);
        assert_eq!(
            decode_chart_setup(&file(&source(), Timestamp::from_nanos(1)).header.options)
                .unwrap()
                .2,
            0
        );
        for length in 0..valid.len() {
            assert!(decode_chart_setup(&valid[..length]).is_err());
        }
        let prefix = b"bms-judge-profile/v3:".len();
        let mut zero_seed = valid.clone();
        zero_seed[prefix..prefix + 8].fill(0);
        assert!(decode_chart_setup(&zero_seed).is_err());
        let mut negative = valid.clone();
        negative[prefix + 8..prefix + 16].copy_from_slice(&(-1_i64).to_le_bytes());
        assert!(decode_chart_setup(&negative).is_err());
        for count in [0_u64, u64::MAX] {
            let mut invalid = valid.clone();
            invalid[prefix + 24..prefix + 32].copy_from_slice(&count.to_le_bytes());
            assert!(decode_chart_setup(&invalid).is_err());
        }
        let mut invalid = valid.clone();
        invalid[prefix + 36..prefix + 44].copy_from_slice(&(-1_i64).to_le_bytes());
        assert!(decode_chart_setup(&invalid).is_err());
        let mut trailing = valid;
        trailing.push(0);
        assert!(decode_chart_setup(&trailing).is_err());
    }

    #[test]
    fn seeded_selected_chart_reconstructs_and_seeks_without_changing_rule_seed() {
        use beatkernel::replay::{ReplayOperation, ReplayRecord};
        use beatkernel_bms::parse_seeded;
        let text =
            "#BPM 120\n#WAV01 head.wav\n#RANDOM 2\n#IF 1\n#00111:01\n#ELSE\n#00112:01\n#ENDIF";
        let selected = parse_seeded(text, ParseOptions::default(), 3).unwrap();
        let other = parse_seeded(text, ParseOptions::default(), 0).unwrap();
        let start = Timestamp::from_nanos(1_000_000_000);
        let section = crate::section_start::source_at(&selected, start).unwrap();
        let judge =
            JudgeEngine::new(section.compile().unwrap().chart, section.rules(), profile()).unwrap();
        let mut recorded = LiveReplayCapture::new_at_with_chart_seed(
            &judge,
            ClockDomainId(17),
            limits(),
            start,
            3,
        )
        .unwrap()
        .into_file();
        recorded.records.push(ReplayRecord {
            ordinal: 0,
            song_time: Timestamp::from_nanos(3_000_000_000),
            operation: ReplayOperation::Advance,
        });
        let mut replay = reconstruct(&selected, recorded.clone(), limits()).unwrap();
        let results = replay.results().to_vec();
        assert_eq!(results.len(), 1);
        let hash = replay.engine().stable_hash().unwrap();
        replay.seek_cursor(0).unwrap();
        assert!(replay.results().is_empty());
        replay.seek_cursor(1).unwrap();
        assert_eq!(replay.results(), results);
        assert_eq!(replay.engine().stable_hash().unwrap(), hash);
        assert!(matches!(
            reconstruct(&other, recorded.clone(), limits()),
            Err(PlaybackError::IdentityMismatch(_))
        ));
        let mut rule_seed = recorded;
        rule_seed.header.seed = 3;
        assert!(matches!(
            reconstruct(&selected, rule_seed, limits()),
            Err(PlaybackError::IdentityMismatch("BMS rule seed"))
        ));
    }

    #[test]
    fn utf8_bom_shift_jis_lnobj_live_reports_and_replay_share_identity_and_timing() {
        use beatkernel::{
            audio::command_queue,
            input::{
                Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
                PhysicalControlId, PhysicalInputEvent,
            },
            judge::{JudgeOutcome, JudgeStage},
            runtime::Runtime,
            time::{ClockMapper, ClockMappingQuality, ClockPoint},
            transport::{Rate, Transport},
        };
        struct Identity;
        impl ClockMapper for Identity {
            fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
                (from.domain == to).then_some(from.timestamp)
            }
            fn quality(&self) -> ClockMappingQuality {
                ClockMappingQuality::Exact
            }
        }
        let point = |at| ClockPoint {
            domain: ClockDomainId(17),
            timestamp: Timestamp::from_nanos(at),
        };
        let text = "#BPM 60\n#BPM01 120\n#STOP01 48\n#LNOBJ ZZ\n#TITLE 日本\n#ARTIST 日本\n#WAV01 日本.wav\n#00011:010000ZZ\n#00008:00010000\n#00009:00010000\n";
        let mut shift_jis = Vec::new();
        for (index, ascii) in text.split("日本").enumerate() {
            if index != 0 {
                shift_jis.extend_from_slice(&[0x93, 0xfa, 0x96, 0x7b]);
            }
            shift_jis.extend_from_slice(ascii.as_bytes());
        }
        let mut bom = vec![0xef, 0xbb, 0xbf];
        bom.extend_from_slice(text.as_bytes());
        let mut reference: Option<ReplayFile> = None;
        for encoded in [text.as_bytes().to_vec(), bom, shift_jis] {
            let decoded = crate::chart_text::decode_chart_text(
                &encoded,
                crate::chart_text::ChartTextEncoding::Auto,
                ParseOptions::default().max_bytes,
            )
            .unwrap();
            assert_eq!(decoded, text);

            let source = parse(&decoded, ParseOptions::default()).unwrap();
            let compiled = source.compile().unwrap();
            let object = &compiled.chart.objects()[0];
            assert_eq!(object.time.start, Timestamp::ZERO);
            assert_eq!(object.time.end, Some(Timestamp::from_nanos(2_500_000_000)));
            let rules = source.rules();
            let control = source.notes[0].lane.control();
            let bindings = BindingMap::from_bindings(rules.iter().map(|rule| Binding {
                device: DeviceSelector::Exact(DeviceId(3)),
                physical: PhysicalControlId::keyboard(rule.control.0 as u16),
                game_control: rule.control,
            }))
            .unwrap();
            let judge = JudgeEngine::new(
                compiled.chart,
                rules,
                JudgeProfile::new(
                    vec![JudgeWindow {
                        grade: JudgeGrade(7),
                        early: Duration::ZERO,
                        late: Duration::ZERO,
                    }],
                    Duration::ZERO,
                )
                .unwrap(),
            )
            .unwrap();
            let mut capture = LiveReplayCapture::new(&judge, ClockDomainId(17), limits()).unwrap();
            let (producer, _consumer) = command_queue(1).unwrap();
            let mut runtime = Runtime::new(
                ClockDomainId(17),
                ClockDomainId(17),
                Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
                bindings,
                judge,
                producer,
                vec![],
                0,
            )
            .unwrap();
            let mut live = Vec::new();
            for (sequence, (at, state)) in
                [(0, ButtonState::Down), (2_500_000_000, ButtonState::Up)]
                    .into_iter()
                    .enumerate()
            {
                let report = runtime
                    .process_input(
                        PhysicalInputEvent::Button(ButtonEvent {
                            meta: EventMeta::new(DeviceId(3), point(at), sequence as u64),
                            control: PhysicalControlId::keyboard(control.0 as u16),
                            state,
                        }),
                        &Identity,
                        point(at),
                    )
                    .unwrap();
                assert!(report.judge_error.is_none());
                live.extend_from_slice(&report.judge_events);
                capture.record_report(&report).unwrap();
            }
            let report = runtime
                .advance_to(point(2_500_000_001), &Identity, point(2_500_000_001))
                .unwrap();
            assert!(report.judge_error.is_none());
            live.extend_from_slice(&report.judge_events);
            capture.record_report(&report).unwrap();
            assert_eq!(live.len(), 2);
            assert_eq!(
                live.iter().map(|event| event.stage).collect::<Vec<_>>(),
                [JudgeStage::HoldHead, JudgeStage::HoldTail]
            );
            assert!(live.iter().all(|event| event.outcome
                == JudgeOutcome::Hit {
                    grade: JudgeGrade(7),
                    delta: Duration::ZERO,
                }));
            let recorded = capture.into_file();
            if let Some(reference) = &reference {
                assert_eq!(recorded.header, reference.header);
                assert_eq!(recorded.records, reference.records);
            } else {
                reference = Some(recorded.clone());
            }

            let mut replay = reconstruct(&source, recorded.clone(), limits()).unwrap();
            assert_eq!(replay.results(), live);
            assert_eq!(
                replay.engine().stable_hash().unwrap(),
                runtime.judge().stable_hash().unwrap()
            );
            replay.seek_cursor(0).unwrap();
            assert!(replay.results().is_empty());
            replay.seek_cursor(recorded.records.len()).unwrap();
            assert_eq!(replay.results(), live);
            assert_eq!(
                replay.engine().stable_hash().unwrap(),
                runtime.judge().stable_hash().unwrap()
            );
            let changed = parse(
                &text.replace("#STOP01 48", "#STOP01 96"),
                ParseOptions::default(),
            )
            .unwrap();
            assert!(matches!(
                reconstruct(&changed, recorded, limits()),
                Err(PlaybackError::IdentityMismatch(_))
            ));
        }
    }

    #[test]
    fn v1_defaults_zero_and_v2_preserves_start_profile_and_signed_offset() {
        let source = source();
        for start in [
            Timestamp::ZERO,
            Timestamp::from_nanos(1),
            Timestamp::from_nanos(1_500_000_000),
        ] {
            let recorded = file(&source, start);
            let (decoded, decoded_start) = decode_setup(&recorded.header.options).unwrap();
            assert_eq!(decoded, profile());
            assert_eq!(decoded_start, start);
            assert_eq!(decode_profile(&recorded.header.options).unwrap(), profile());
            let expected_header = recorded.header.clone();
            let pristine = validate_setup(&source, &recorded, limits()).unwrap();
            assert_eq!(pristine.effective_song_time(), None);
            assert_eq!(recorded.header, expected_header);
            assert!(recorded.records.is_empty());
            let filtered = crate::section_start::source_at(&source, start).unwrap();
            assert!(reconstruct(&filtered, recorded.clone(), limits()).is_ok());
            assert!(reconstruct(&source, recorded, limits()).is_ok());
        }
    }

    #[test]
    fn v2_rejects_nonpositive_truncated_unknown_and_bad_body_metadata() {
        let valid = file(&source(), Timestamp::from_nanos(1)).header.options;
        for length in 0..valid.len() {
            assert!(decode_setup(&valid[..length]).is_err());
        }
        for start in [0_i64, -1, i64::MIN] {
            let mut malformed = valid.clone();
            let offset = b"bms-judge-profile/v2:".len();
            malformed[offset..offset + 8].copy_from_slice(&start.to_le_bytes());
            assert!(decode_setup(&malformed).is_err());
        }
        let mut trailing = valid.clone();
        trailing.push(0);
        assert!(decode_setup(&trailing).is_err());
        let mut unknown = valid.clone();
        unknown[b"bms-judge-profile/v".len()] = b'4';
        assert!(decode_setup(&unknown).is_err());
        for count in [0_u64, u64::MAX] {
            let mut malformed = valid.clone();
            let offset = b"bms-judge-profile/v2:".len() + 16;
            malformed[offset..offset + 8].copy_from_slice(&count.to_le_bytes());
            assert!(decode_setup(&malformed).is_err());
        }
        let mut bad_window = valid;
        let early = b"bms-judge-profile/v2:".len() + 8 + 16 + 4;
        bad_window[early..early + 8].copy_from_slice(&(-1_i64).to_le_bytes());
        assert!(decode_setup(&bad_window).is_err());
    }

    #[test]
    fn reconstruction_matches_filtered_identity_and_preserves_canonical_checks() {
        let source = source();
        let original = file(&source, Timestamp::ZERO);
        let section = file(&source, Timestamp::from_nanos(1));
        let later_same_heads = file(&source, Timestamp::from_nanos(2));
        assert_ne!(
            original.header.chart_identity,
            section.header.chart_identity
        );
        assert_eq!(
            section.header.chart_identity,
            later_same_heads.header.chart_identity
        );
        assert_ne!(section.header.options, later_same_heads.header.options);
        assert!(!crate::competition::compatible_headers(
            &section.header,
            &later_same_heads.header
        ));
        assert!(crate::competition::compatible_headers(
            &section.header,
            &section.header
        ));
        let mut full_profile_on_section = section.clone();
        full_profile_on_section.header.options = original.header.options;
        assert!(matches!(
            reconstruct(&source, full_profile_on_section, limits()),
            Err(PlaybackError::IdentityMismatch(_))
        ));
        let mut variants = Vec::new();
        let mut changed = section.clone();
        changed.header.seed = 1;
        variants.push(changed);
        let mut changed = section.clone();
        changed.header.rules_identity.push(0);
        variants.push(changed);
        let mut changed = section.clone();
        changed.runtime_version.push('x');
        variants.push(changed);
        let mut changed = section.clone();
        changed.header.chart_identity[0] ^= 1;
        variants.push(changed);
        for changed in variants {
            assert!(reconstruct(&source, changed, limits()).is_err());
        }
        let changed = parse(
            "#BPM 120\n#WAV01 head.wav\n#00011:01\n#00113:01\n",
            ParseOptions::default(),
        )
        .unwrap();
        assert!(matches!(
            reconstruct(&changed, section, limits()),
            Err(PlaybackError::IdentityMismatch(_))
        ));
    }
}
