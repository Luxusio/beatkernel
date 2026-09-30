//! Application identity checks and logical reconstruction of captured BMS play.

use crate::replay_capture::{CaptureError, LiveReplayCapture};
use beatkernel::{
    judge::{JudgeEngine, JudgeError, JudgeGrade, JudgeProfile, JudgeWindow},
    replay::{
        codec::{decode_replay, encode_replay, ReplayCodecError, ReplayCodecLimits, ReplayFile},
        ReplayError, ReplaySession,
    },
    time::Duration,
};
use beatkernel_bms::{BmsChart, BmsError};
use std::io::{ErrorKind, Read};

/// Invalid replay metadata, compatibility, bounded data or logical reconstruction.
#[derive(Debug)]
pub enum PlaybackError {
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

/// Decodes the exact versioned profile captured by the BMS runtime.
///
/// Extent checks precede window allocation. The resulting profile applies its
/// signed offset once through JudgeEngine, never by editing recorded times.
pub fn decode_profile(options: &[u8]) -> Result<JudgeProfile, PlaybackError> {
    let bytes = options
        .strip_prefix(b"bms-judge-profile/v1:")
        .ok_or(PlaybackError::Metadata("unsupported profile schema"))?;
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
    Ok(JudgeProfile::new(windows, Duration::from_nanos(offset))?)
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
    // Also validates files assembled directly by callers, not only decoded logs.
    encode_replay(&file, limits)?;
    if file.runtime_version != env!("CARGO_PKG_VERSION") {
        return Err(PlaybackError::IdentityMismatch("runtime version"));
    }
    if file.header.rules_identity != b"beatkernel-bms/builtin-judge/v1" {
        return Err(PlaybackError::IdentityMismatch("BMS rule schema"));
    }
    if file.header.seed != 0 {
        return Err(PlaybackError::IdentityMismatch("BMS rule seed"));
    }
    let profile = decode_profile(&file.header.options)?;
    let compiled = source.compile()?;
    let judge = JudgeEngine::new(compiled.chart, source.rules(), profile)?;
    let expected = LiveReplayCapture::new(&judge, file.header.normalized_clock, limits)?;
    if expected.header() != &file.header {
        return Err(PlaybackError::IdentityMismatch(
            "compiled judge setup/profile",
        ));
    }
    Ok(ReplaySession::from_records(
        file.header,
        judge,
        file.records,
    )?)
}
