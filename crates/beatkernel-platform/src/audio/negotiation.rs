use super::{
    AudioPlatformError, AudioStreamMode, AudioStreamRequest, BufferRequest,
    ConfigurationConstraint, NegotiationPolicy, PeriodConstraints, PeriodRequest,
    SharedPeriodPolicy,
};
use beatkernel::time::Duration;

/// Checked applied period, with explicit caller-authorized adjustment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedPeriod {
    /// Applied frame count.
    pub frames: u32,
    /// Integer nanoseconds, rounded upward only for reporting frame duration.
    pub duration: Duration,
    /// Whether caller-selected period sizing was changed.
    pub adjusted: bool,
}

/// Resolves explicit period sizing against reported native constraints.
///
/// Unknown bounds stay unknown. Exact duration requests must be an exact
/// rational frame duration; opt-in sizing rounds upward to a supported multiple.
pub fn resolve_period(
    request: &AudioStreamRequest,
    constraints: PeriodConstraints,
) -> Result<ResolvedPeriod, AudioPlatformError> {
    if matches!(
        request.mode(),
        AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault)
    ) && request.period() != PeriodRequest::DeviceDefault
    {
        return Err(constraint(
            ConfigurationConstraint::LegacyDeviceDefaultPeriod,
            constraints,
            constraints.default_frames,
        ));
    }
    let rate = request.format().sample_rate();
    let (wanted, exact_duration) = match request.period() {
        PeriodRequest::Frames(frames) => (frames, true),
        PeriodRequest::Duration(duration) => duration_frames(duration, rate)?,
        PeriodRequest::DeviceDefault => match constraints.default_frames {
            Some(frames) => (frames, true),
            None => duration_frames(
                constraints.default_period.ok_or_else(|| {
                    constraint(ConfigurationConstraint::PeriodBounds, constraints, None)
                })?,
                rate,
            )?,
        },
    };
    if request.mode() == AudioStreamMode::Exclusive {
        let matches = match request.buffer() {
            BufferRequest::DeviceDefault => true,
            BufferRequest::Frames(buffer) => buffer == wanted,
            BufferRequest::Duration(duration) => match request.period() {
                PeriodRequest::Duration(period) => duration == period,
                _ => {
                    i128::from(duration.as_nanos()) * i128::from(rate)
                        == i128::from(wanted) * 1_000_000_000
                }
            },
        };
        if !matches {
            return Err(constraint(
                ConfigurationConstraint::ExclusiveBufferEqualsPeriod,
                constraints,
                Some(wanted),
            ));
        }
    }
    let mut minimum = constraints.min_frames.unwrap_or(1).max(1);
    let mut maximum = constraints.max_frames.unwrap_or(u32::MAX);
    if let Some(duration) = constraints.min_period {
        minimum = minimum.max(duration_frames(duration, rate)?.0);
    }
    if let Some(duration) = constraints.max_period {
        let product = i128::from(duration.as_nanos()) * i128::from(rate);
        let floor = u32::try_from(product / 1_000_000_000)
            .map_err(|_| AudioPlatformError::InvalidRequest)?;
        maximum = maximum.min(floor);
    }
    let mut multiple = constraints.fundamental_frames.unwrap_or(1);
    if let (AudioStreamMode::Exclusive, Some(alignment)) =
        (request.mode(), constraints.alignment_frames)
    {
        if alignment == 0 || multiple == 0 {
            return Err(constraint(
                ConfigurationConstraint::PeriodBounds,
                constraints,
                None,
            ));
        }
        multiple =
            u32::try_from(u64::from(multiple / gcd(multiple, alignment)) * u64::from(alignment))
                .map_err(|_| {
                    constraint(ConfigurationConstraint::PeriodBounds, constraints, None)
                })?;
    }
    if multiple == 0 || minimum > maximum || wanted == 0 {
        return Err(constraint(
            ConfigurationConstraint::PeriodBounds,
            constraints,
            None,
        ));
    }
    let candidate =
        u64::from(wanted.max(minimum)).div_ceil(u64::from(multiple)) * u64::from(multiple);
    let lowest = u64::from(minimum).div_ceil(u64::from(multiple)) * u64::from(multiple);
    let highest = maximum / multiple * multiple;
    let suggested =
        (lowest <= u64::from(highest)).then_some(candidate.min(u64::from(highest)) as u32);
    let adjusted = !exact_duration || suggested != Some(wanted);
    // DeviceDefault explicitly accepts its native integer reporting resolution.
    if suggested.is_none()
        // A smaller supported maximum is advisory, never an upward rounding.
        || candidate > u64::from(maximum)
        || (adjusted
            && request.period() != PeriodRequest::DeviceDefault
            && request.negotiation() == NegotiationPolicy::Exact)
    {
        return Err(AudioPlatformError::ConfigurationUnsupported {
            constraint: ConfigurationConstraint::PeriodBounds,
            constraints,
            // Exclusive event streams must resize buffer and period together.
            suggested_buffer_frames: if request.mode() == AudioStreamMode::Exclusive {
                suggested
            } else {
                None
            },
            suggested_period_frames: suggested,
        });
    }
    let frames = suggested.expect("checked period suggestion");
    Ok(ResolvedPeriod {
        frames,
        duration: frame_duration(frames, rate),
        adjusted: adjusted && request.period() != PeriodRequest::DeviceDefault,
    })
}

/// Checks reported native buffer capacity without silently authorizing changes.
pub fn validate_buffer_size(
    request: &AudioStreamRequest,
    actual_frames: u32,
) -> Result<bool, AudioPlatformError> {
    if actual_frames == 0 {
        return Err(AudioPlatformError::InvalidRequest);
    }
    let matches = match request.buffer() {
        BufferRequest::DeviceDefault => return Ok(false),
        BufferRequest::Frames(frames) => frames == actual_frames,
        BufferRequest::Duration(duration) => {
            i128::from(duration.as_nanos()) * i128::from(request.format().sample_rate())
                == i128::from(actual_frames) * 1_000_000_000
        }
    };
    if matches {
        return Ok(false);
    }
    if request.negotiation() == NegotiationPolicy::AllowSupportedRounding {
        return Ok(true);
    }
    Err(AudioPlatformError::ConfigurationUnsupported {
        constraint: match request.mode() {
            AudioStreamMode::Exclusive => ConfigurationConstraint::BufferAlignment,
            AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod) => {
                ConfigurationConstraint::EngineManagedBuffer
            }
            AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault) => {
                ConfigurationConstraint::BufferSize
            }
        },
        constraints: PeriodConstraints::default(),
        suggested_buffer_frames: Some(actual_frames),
        suggested_period_frames: None,
    })
}

fn duration_frames(duration: Duration, rate: u32) -> Result<(u32, bool), AudioPlatformError> {
    if duration.as_nanos() <= 0 {
        return Err(AudioPlatformError::InvalidRequest);
    }
    let product = i128::from(duration.as_nanos()) * i128::from(rate);
    let quotient = product / 1_000_000_000;
    let remainder = product % 1_000_000_000;
    let frames = u32::try_from(quotient + i128::from(remainder != 0))
        .map_err(|_| AudioPlatformError::InvalidRequest)?;
    Ok((frames, remainder == 0))
}

pub(super) fn frame_duration(frames: u32, rate: u32) -> Duration {
    let nanos = (u64::from(frames) * 1_000_000_000).div_ceil(u64::from(rate));
    Duration::from_nanos(nanos as i64)
}

fn constraint(
    constraint: ConfigurationConstraint,
    constraints: PeriodConstraints,
    suggestion: Option<u32>,
) -> AudioPlatformError {
    AudioPlatformError::ConfigurationUnsupported {
        constraint,
        constraints,
        suggested_buffer_frames: (constraint
            == ConfigurationConstraint::ExclusiveBufferEqualsPeriod)
            .then_some(suggestion)
            .flatten(),
        suggested_period_frames: suggestion,
    }
}

fn gcd(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}
