//! Canonical optional class identity around complete application setup options.
use crate::{
    judgment_policy::{BmsJudgmentPolicy, GradeClass, JudgmentPolicyError},
    gauge::MAX_GAUGE_GRADES,
};
use beatkernel::{
    judge::JudgeGrade,
    replay::{ReplayHeader, codec::ReplayCodecLimits},
};
use beatkernel_bms::BmsJudgment;
pub(crate) const PREFIX: &[u8] = b"bms-judgment-setup/v1:";
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    Invalid(&'static str),
    Classes(JudgmentPolicyError),
    HeaderTooLarge,
    AllocationFailed,
}
impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "replay judgment policy: {self:?}")
    }
}
impl std::error::Error for PolicyError {}
pub fn wrap_header(
    mut header: ReplayHeader,
    policy: Option<&BmsJudgmentPolicy>,
    limits: ReplayCodecLimits,
) -> Result<ReplayHeader, PolicyError> {
    if header.options.starts_with(PREFIX) {
        return Err(PolicyError::Invalid("nested judgment setup"));
    }
    let Some(policy) = policy else {
        return Ok(header);
    };
    let inner_len = u32::try_from(header.options.len()).map_err(|_| PolicyError::HeaderTooLarge)?;
    let size = PREFIX
        .len()
        .checked_add(5)
        .and_then(|n| n.checked_add(header.options.len()))
        .and_then(|n| n.checked_add(policy.entries().len() * 5))
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
    options.push(policy.entries().len() as u8);
    for entry in policy.entries() {
        options.extend_from_slice(&entry.grade.0.to_le_bytes());
        options.push(match entry.class {
            BmsJudgment::PGreat => 0,
            BmsJudgment::Great => 1,
            BmsJudgment::Good => 2,
            BmsJudgment::Bad => 3,
            _ => unreachable!("validated hit classes"),
        });
    }
    header.options = options;
    Ok(header)
}
/// Fixed bounded storage; every extent and canonical order checked before use.
pub fn split_options(options: &[u8]) -> Result<(&[u8], Option<BmsJudgmentPolicy>), PolicyError> {
    let Some(bytes) = options.strip_prefix(PREFIX) else {
        return Ok((options, None));
    };
    let len = u32::from_le_bytes(
        bytes
            .get(..4)
            .ok_or(PolicyError::Invalid("truncated inner length"))?
            .try_into()
            .unwrap(),
    ) as usize;
    let end = 4usize
        .checked_add(len)
        .ok_or(PolicyError::Invalid("inner extent overflow"))?;
    let inner = bytes
        .get(4..end)
        .ok_or(PolicyError::Invalid("truncated inner setup"))?;
    if inner.starts_with(PREFIX) {
        return Err(PolicyError::Invalid("nested judgment setup"));
    }
    let count = usize::from(
        *bytes
            .get(end)
            .ok_or(PolicyError::Invalid("truncated class count"))?,
    );
    let classes = bytes
        .get(
            end.checked_add(1)
                .ok_or(PolicyError::Invalid("class extent overflow"))?..,
        )
        .ok_or(PolicyError::Invalid("truncated classes"))?;
    if count == 0 || count > MAX_GAUGE_GRADES || classes.len() != count * 5 {
        return Err(PolicyError::Invalid("class extent"));
    }
    let mut entries = [GradeClass {
        grade: JudgeGrade(0),
        class: BmsJudgment::Bad,
    }; MAX_GAUGE_GRADES];
    for (index, bytes) in classes.chunks_exact(5).enumerate() {
        let grade = JudgeGrade(u32::from_le_bytes(bytes[..4].try_into().unwrap()));
        if index != 0 && entries[index - 1].grade.0 >= grade.0 {
            return Err(PolicyError::Invalid("class grade order"));
        }
        entries[index] = GradeClass {
            grade,
            class: match bytes[4] {
                0 => BmsJudgment::PGreat,
                1 => BmsJudgment::Great,
                2 => BmsJudgment::Good,
                3 => BmsJudgment::Bad,
                _ => return Err(PolicyError::Invalid("hit class tag")),
            },
        };
    }
    Ok((
        inner,
        Some(BmsJudgmentPolicy::new(&entries[..count]).map_err(PolicyError::Classes)?),
    ))
}
