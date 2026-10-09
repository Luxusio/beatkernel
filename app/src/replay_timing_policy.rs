//! Canonical optional timing identity outside judgment, gauge and profile setup.
use crate::play_policy::{ResolvedTimingPolicy, TimingPresetSelection};
use beatkernel::replay::{codec::ReplayCodecLimits, ReplayHeader};
use beatkernel_bms::{
    BmsDefExRank, BmsJudgeDifficulty, BmsJudgment, BmsRank, BmsRankPrecedence, BmsTimingPreset,
    BmsTimingStage,
};

pub(crate) const PREFIX: &[u8] = b"bms-timing-setup/v1:";
const FAMILY: &[u8] = b"bms-timing-setup/";
const SEMANTICS: &[u8] = b"beatkernel-hold/v1";
const STAGES: [BmsTimingStage; 4] = [
    BmsTimingStage::KeyHead,
    BmsTimingStage::ScratchHead,
    BmsTimingStage::KeyTail,
    BmsTimingStage::ScratchTail,
];

#[derive(Debug)]
pub enum PolicyError {
    Invalid(&'static str),
    Policy(crate::play_policy::PolicyError),
    HeaderTooLarge,
    AllocationFailed,
}
impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "replay timing policy: {self:?}")
    }
}
impl std::error::Error for PolicyError {}

/// Adds the outermost immutable timing policy; absent selection preserves bytes.
pub fn wrap_header(
    mut header: ReplayHeader,
    policy: Option<&ResolvedTimingPolicy>,
    limits: ReplayCodecLimits,
) -> Result<ReplayHeader, PolicyError> {
    let Some(policy) = policy else {
        return Ok(header);
    };
    if header.options.starts_with(FAMILY) {
        return Err(PolicyError::Invalid("nested timing setup"));
    }
    let id = policy.selection().preset.id().as_bytes();
    let semantics = policy.interaction_semantics().as_bytes();
    let difficulty_len = match policy.profiles().difficulty() {
        BmsJudgeDifficulty::Rank(_) => 1,
        BmsJudgeDifficulty::DefExRank(_) => 16,
    };
    let inner_len = u32::try_from(header.options.len()).map_err(|_| PolicyError::HeaderTooLarge)?;
    // Fixed four stages of four classified windows; no caller-defined counts.
    let fixed =
        PREFIX.len() + 4 + 1 + id.len() + 1 + semantics.len() + 2 + difficulty_len + 16 + 16 * 21;
    let size = fixed
        .checked_add(header.options.len())
        .ok_or(PolicyError::HeaderTooLarge)?;
    let total = size
        .checked_add(header.chart_identity.len())
        .and_then(|n| n.checked_add(header.rules_identity.len()))
        .and_then(|n| n.checked_add(env!("CARGO_PKG_VERSION").len()))
        .ok_or(PolicyError::HeaderTooLarge)?;
    if total > limits.max_header_bytes() {
        return Err(PolicyError::HeaderTooLarge);
    }
    let mut options = Vec::new();
    options
        .try_reserve_exact(size)
        .map_err(|_| PolicyError::AllocationFailed)?;
    options.extend_from_slice(PREFIX);
    options.extend_from_slice(&inner_len.to_le_bytes());
    options.extend_from_slice(&header.options);
    options.push(id.len() as u8);
    options.extend_from_slice(id);
    options.push(semantics.len() as u8);
    options.extend_from_slice(semantics);
    options.push(match policy.selection().precedence {
        BmsRankPrecedence::RankFirst => 0,
        BmsRankPrecedence::DefExRankFirst => 1,
    });
    match policy.profiles().difficulty() {
        BmsJudgeDifficulty::Rank(rank) => {
            options.push(0);
            options.push(rank.code());
        }
        BmsJudgeDifficulty::DefExRank(value) => {
            options.push(1);
            options.extend_from_slice(&value.numerator().to_le_bytes());
        }
    }
    options.extend_from_slice(&policy.profiles().effective_percentage().to_le_bytes());
    for stage in STAGES {
        for entry in policy.profiles().windows(stage) {
            options.extend_from_slice(&entry.window.grade.0.to_le_bytes());
            options.push(class_tag(entry.judgment));
            options.extend_from_slice(&entry.window.early.as_nanos().to_le_bytes());
            options.extend_from_slice(&entry.window.late.as_nanos().to_le_bytes());
        }
    }
    header.options = options;
    Ok(header)
}

fn class_tag(class: BmsJudgment) -> u8 {
    match class {
        BmsJudgment::PGreat => 0,
        BmsJudgment::Great => 1,
        BmsJudgment::Good => 2,
        BmsJudgment::Bad => 3,
        _ => unreachable!("validated timing hit classes"),
    }
}

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], PolicyError> {
        let bytes = self
            .0
            .get(..len)
            .ok_or(PolicyError::Invalid("truncated timing setup"))?;
        self.0 = &self.0[len..];
        Ok(bytes)
    }
    fn byte(&mut self) -> Result<u8, PolicyError> {
        Ok(self.take(1)?[0])
    }
    fn text(&mut self) -> Result<&'a [u8], PolicyError> {
        let len = usize::from(self.byte()?);
        self.take(len)
    }
}

/// Reconstructs the selected table and checks every recorded effective window.
/// Unwrapped legacy options borrow their original bytes without allocation.
pub fn split_options(options: &[u8]) -> Result<(&[u8], Option<ResolvedTimingPolicy>), PolicyError> {
    let Some(bytes) = options.strip_prefix(PREFIX) else {
        if options.starts_with(FAMILY) {
            return Err(PolicyError::Invalid("unknown timing setup version"));
        }
        return Ok((options, None));
    };
    let mut reader = Reader(bytes);
    let inner_len = u32::from_le_bytes(reader.take(4)?.try_into().unwrap()) as usize;
    let inner = reader.take(inner_len)?;
    if inner.starts_with(FAMILY) {
        return Err(PolicyError::Invalid("nested timing setup"));
    }
    let preset = BmsTimingPreset::BeatorajaSevenKeys8320241dV1;
    if reader.text()? != preset.id().as_bytes() {
        return Err(PolicyError::Invalid("unknown timing preset version"));
    }
    if reader.text()? != SEMANTICS {
        return Err(PolicyError::Invalid("unknown interaction semantics"));
    }
    let precedence = match reader.byte()? {
        0 => BmsRankPrecedence::RankFirst,
        1 => BmsRankPrecedence::DefExRankFirst,
        _ => return Err(PolicyError::Invalid("rank precedence tag")),
    };
    let difficulty = match reader.byte()? {
        0 => BmsJudgeDifficulty::Rank(match reader.byte()? {
            0 => BmsRank::VeryHard,
            1 => BmsRank::Hard,
            2 => BmsRank::Normal,
            3 => BmsRank::Easy,
            4 => BmsRank::VeryEasy,
            _ => return Err(PolicyError::Invalid("rank code")),
        }),
        1 => {
            let numerator = i128::from_le_bytes(reader.take(16)?.try_into().unwrap());
            if numerator <= 0 {
                return Err(PolicyError::Invalid("positive integer DEFEXRANK required"));
            }
            let value = BmsDefExRank::parse(&numerator.to_string())
                .map_err(|_| PolicyError::Invalid("DEFEXRANK precision"))?;
            BmsJudgeDifficulty::DefExRank(value)
        }
        _ => return Err(PolicyError::Invalid("difficulty source tag")),
    };
    let policy = ResolvedTimingPolicy::from_recorded(
        TimingPresetSelection { preset, precedence },
        difficulty,
    )
    .map_err(PolicyError::Policy)?;
    let percentage = i128::from_le_bytes(reader.take(16)?.try_into().unwrap());
    if percentage != policy.profiles().effective_percentage() {
        return Err(PolicyError::Invalid("effective percentage mismatch"));
    }
    for stage in STAGES {
        for expected in policy.profiles().windows(stage) {
            let grade = u32::from_le_bytes(reader.take(4)?.try_into().unwrap());
            let class = reader.byte()?;
            let early = i64::from_le_bytes(reader.take(8)?.try_into().unwrap());
            let late = i64::from_le_bytes(reader.take(8)?.try_into().unwrap());
            if grade != expected.window.grade.0
                || class != class_tag(expected.judgment)
                || early != expected.window.early.as_nanos()
                || late != expected.window.late.as_nanos()
            {
                return Err(PolicyError::Invalid("effective stage window mismatch"));
            }
        }
    }
    if !reader.0.is_empty() {
        return Err(PolicyError::Invalid("trailing timing setup bytes"));
    }
    Ok((inner, Some(policy)))
}
