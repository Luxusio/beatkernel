//! Canonical application replay options retaining a resolved gauge policy.
use crate::gauge::{GaugeDynamics, GaugeProfile, GradeDelta, MAX_GAUGE_GRADES};
use beatkernel::{
    judge::JudgeGrade,
    replay::{ReplayHeader, codec::ReplayCodecLimits},
};

const PREFIX: &[u8] = b"bms-gauge-setup/v1:";
const FIXED: usize = 61;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    Invalid(&'static str),
    AllocationFailed,
    HeaderTooLarge,
}
impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "replay gauge policy: {self:?}")
    }
}
impl std::error::Error for PolicyError {}

/// Default profiles leave the header byte-for-byte unchanged.
pub fn wrap_header(
    mut header: ReplayHeader,
    profile: &GaugeProfile,
    limits: ReplayCodecLimits,
) -> Result<ReplayHeader, PolicyError> {
    if header.options.starts_with(PREFIX) {
        return Err(PolicyError::Invalid("nested gauge setup"));
    }
    if profile == &GaugeProfile::default() {
        return Ok(header);
    }
    let judge_len = u32::try_from(header.options.len()).map_err(|_| PolicyError::HeaderTooLarge)?;
    let size = PREFIX
        .len()
        .checked_add(4)
        .and_then(|n| n.checked_add(header.options.len()))
        .and_then(|n| n.checked_add(FIXED + profile.grades().len() * 12))
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
    options.extend_from_slice(&judge_len.to_le_bytes());
    options.extend_from_slice(&header.options);
    for bytes in [
        profile.initial_units().to_le_bytes(),
        profile.clear_units().to_le_bytes(),
        profile.default_hit_delta().to_le_bytes(),
        profile.miss_delta().to_le_bytes(),
    ] {
        options.extend_from_slice(&bytes);
    }
    options.push(u8::from(profile.fail_on_empty()));
    options.extend_from_slice(&(profile.grades().len() as u32).to_le_bytes());
    let dynamics = profile.dynamics();
    for value in [
        dynamics.minimum_alive,
        dynamics.failure_below,
        dynamics.damage_reduction_below,
    ] {
        options.extend_from_slice(&value.to_le_bytes());
    }
    for grade in profile.grades() {
        options.extend_from_slice(&grade.grade.0.to_le_bytes());
        options.extend_from_slice(&grade.delta.to_le_bytes());
    }
    header.options = options;
    Ok(header)
}

/// Checks all extents before allocating bounded grade storage.
pub fn split_options(options: &[u8]) -> Result<(&[u8], GaugeProfile), PolicyError> {
    let Some(bytes) = options.strip_prefix(PREFIX) else {
        return Ok((options, GaugeProfile::default()));
    };
    let encoded_len = bytes
        .get(..4)
        .ok_or(PolicyError::Invalid("truncated judge length"))?;
    let judge_len = u32::from_le_bytes(encoded_len.try_into().unwrap()) as usize;
    let end = 4usize
        .checked_add(judge_len)
        .ok_or(PolicyError::Invalid("judge extent overflow"))?;
    let judge = bytes
        .get(4..end)
        .ok_or(PolicyError::Invalid("truncated judge options"))?;
    if judge.starts_with(PREFIX) {
        return Err(PolicyError::Invalid("nested gauge setup"));
    }
    let gauge = bytes
        .get(end..)
        .ok_or(PolicyError::Invalid("truncated gauge"))?;
    let fixed = gauge
        .get(..FIXED)
        .ok_or(PolicyError::Invalid("truncated gauge fields"))?;
    let fail = match fixed[32] {
        0 => false,
        1 => true,
        _ => return Err(PolicyError::Invalid("gauge boolean")),
    };
    let count = u32::from_le_bytes(fixed[33..37].try_into().unwrap()) as usize;
    if count > MAX_GAUGE_GRADES || gauge.len() != FIXED + count * 12 {
        return Err(PolicyError::Invalid("gauge grade extent"));
    }
    let read = |offset| u64::from_le_bytes(fixed[offset..offset + 8].try_into().unwrap());
    let dynamics = GaugeDynamics {
        minimum_alive: read(37),
        failure_below: read(45),
        damage_reduction_below: read(53),
    };
    GaugeProfile::new(
        read(0),
        read(8),
        read(16) as i64,
        read(24) as i64,
        fail,
        Vec::new(),
    )
    .and_then(|p| p.with_dynamics(dynamics))
    .map_err(|_| PolicyError::Invalid("gauge profile"))?;
    let mut previous = None;
    for bytes in gauge[FIXED..].chunks_exact(12) {
        let grade = JudgeGrade(u32::from_le_bytes(bytes[..4].try_into().unwrap()));
        if previous.is_some_and(|prior| prior >= grade) {
            return Err(PolicyError::Invalid("gauge grade order"));
        }
        previous = Some(grade);
    }
    let mut grades = Vec::new();
    grades
        .try_reserve_exact(count)
        .map_err(|_| PolicyError::AllocationFailed)?;
    for bytes in gauge[FIXED..].chunks_exact(12) {
        let grade = JudgeGrade(u32::from_le_bytes(bytes[..4].try_into().unwrap()));
        grades.push(GradeDelta {
            grade,
            delta: i64::from_le_bytes(bytes[4..].try_into().unwrap()),
        });
    }
    let profile = GaugeProfile::new(
        read(0),
        read(8),
        read(16) as i64,
        read(24) as i64,
        fail,
        grades,
    )
    .and_then(|p| {
        p.with_dynamics(GaugeDynamics {
            minimum_alive: read(37),
            failure_below: read(45),
            damage_reduction_below: read(53),
        })
    })
    .map_err(|_| PolicyError::Invalid("gauge profile"))?;
    if profile == GaugeProfile::default() {
        return Err(PolicyError::Invalid("redundant default gauge"));
    }
    Ok((judge, profile))
}

#[cfg(test)]
#[path = "replay_gauge_policy_fixtures.rs"]
mod fixtures;
