//! Exact bounded frame decoding for independently validated native phase lanes.

pub const NATIVE_PERIODS: [u32; 3] = [1_048_576, 1_048_575, 1_048_573];
pub const MAX_NATIVE_FRAME: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeClockError {
    Invalid,
    State,
    Chronology,
    Overflow,
}

impl std::fmt::Display for NativeClockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "native clock: {self:?}")
    }
}
impl std::error::Error for NativeClockError {}

/// Combines the fixed lanes without converting their exact integer coordinates.
/// This arithmetic alone does not authenticate a native epoch or callback.
pub fn joint_offset(phases: [u32; 3]) -> Result<u64, NativeClockError> {
    if phases
        .iter()
        .zip(NATIVE_PERIODS)
        .any(|(&phase, period)| phase >= period)
    {
        return Err(NativeClockError::Invalid);
    }
    let [r0, r1, r2] = phases.map(u64::from);
    let [p0, p1, p2] = NATIVE_PERIODS.map(u64::from);
    // p0 is one modulo p1. These modular differences are bounded by two periods.
    let t1 = (r1 + p1 - r0 % p1) % p1;
    let x01 = p0
        .checked_mul(t1)
        .and_then(|value| value.checked_add(r0))
        .ok_or(NativeClockError::Overflow)?;
    // (p0 * p1)^-1 modulo p2 is 873811.
    let t2 = ((r2 + p2 - x01 % p2) % p2)
        .checked_mul(873_811)
        .ok_or(NativeClockError::Overflow)?
        % p2;
    let offset = p0
        .checked_mul(p1)
        .and_then(|value| value.checked_mul(t2))
        .and_then(|value| value.checked_add(x01))
        .ok_or(NativeClockError::Overflow)?;
    if offset > MAX_NATIVE_FRAME {
        return Err(NativeClockError::Overflow);
    }
    Ok(offset)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Admission {
    Unproven,
    First(u64),
    Ready(u64),
}

/// Immutable native origin, whole committed horizons and admitted callback end.
/// The native adapter must still validate every original sample before publishing
/// this candidate, and permanently fence its owner if that validation fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeClock {
    origin: u64,
    horizons: [u64; 3],
    admission: Admission,
}

fn validate_horizons(origin: u64, horizons: [u64; 3]) -> Result<(), NativeClockError> {
    for (horizon, period) in horizons.into_iter().zip(NATIVE_PERIODS) {
        if horizon > MAX_NATIVE_FRAME {
            return Err(NativeClockError::Overflow);
        }
        if horizon <= origin || (horizon - origin) % u64::from(period) != 0 {
            return Err(NativeClockError::Invalid);
        }
    }
    Ok(())
}

impl NativeClock {
    pub fn new(origin: u64, horizons: [u64; 3]) -> Result<Self, NativeClockError> {
        if origin == 0 {
            return Err(NativeClockError::Invalid);
        }
        if origin > MAX_NATIVE_FRAME {
            return Err(NativeClockError::Overflow);
        }
        validate_horizons(origin, horizons)?;
        Ok(Self {
            origin,
            horizons,
            admission: Admission::Unproven,
        })
    }

    fn common_horizon(&self) -> u64 {
        self.horizons[0].min(self.horizons[1]).min(self.horizons[2])
    }

    /// Returns the original callback base. Invalid admission leaves state intact.
    /// Forward gaps after readiness remain visible to the strict armed renderer.
    pub fn frame(
        &mut self,
        first: [u32; 3],
        last: [u32; 3],
        first_index: u32,
        frames: u32,
    ) -> Result<u64, NativeClockError> {
        let valid_index = match self.admission {
            Admission::Unproven => first_index > 0,
            Admission::First(_) | Admission::Ready(_) => first_index == 0,
        };
        if frames == 0 || first_index >= frames || !valid_index {
            return Err(NativeClockError::Invalid);
        }
        let first_offset = joint_offset(first)?;
        let last_offset = joint_offset(last)?;
        let active_frames = u64::from(frames - first_index);
        let expected_last = first_offset
            .checked_add(active_frames - 1)
            .filter(|&value| value <= MAX_NATIVE_FRAME)
            .ok_or(NativeClockError::Overflow)?;
        if last_offset != expected_last {
            return Err(NativeClockError::Chronology);
        }
        let base = match self.admission {
            Admission::Unproven => {
                if first_offset != 0 {
                    return Err(NativeClockError::Invalid);
                }
                self.origin
                    .checked_sub(u64::from(first_index))
                    .ok_or(NativeClockError::Overflow)?
            }
            Admission::First(_) | Admission::Ready(_) => self
                .origin
                .checked_add(first_offset)
                .filter(|&value| value <= MAX_NATIVE_FRAME)
                .ok_or(NativeClockError::Overflow)?,
        };
        let end = base
            .checked_add(u64::from(frames))
            .filter(|&value| value <= MAX_NATIVE_FRAME)
            .ok_or(NativeClockError::Overflow)?;
        if end > self.common_horizon() {
            return Err(NativeClockError::Chronology);
        }
        match self.admission {
            Admission::First(previous_end) if base != previous_end => {
                return Err(NativeClockError::Chronology);
            }
            Admission::Ready(previous_end) if base < previous_end => {
                return Err(NativeClockError::Chronology);
            }
            _ => {}
        }
        self.admission = match self.admission {
            Admission::Unproven => Admission::First(end),
            Admission::First(_) | Admission::Ready(_) => Admission::Ready(end),
        };
        Ok(base)
    }

    pub fn ready(&self) -> bool {
        matches!(self.admission, Admission::Ready(_))
    }

    /// Commits all own-period extensions together before the observed horizon.
    /// Invalid geometry, unreadiness or expiry preserves the previous state.
    pub fn extend(&mut self, horizons: [u64; 3]) -> Result<(), NativeClockError> {
        let end = match self.admission {
            Admission::Ready(end) => end,
            Admission::Unproven | Admission::First(_) => return Err(NativeClockError::State),
        };
        if end >= self.common_horizon() {
            return Err(NativeClockError::Chronology);
        }
        validate_horizons(self.origin, horizons)?;
        for ((new, old), period) in horizons.into_iter().zip(self.horizons).zip(NATIVE_PERIODS) {
            if new <= old || (new - old) % u64::from(period) != 0 {
                return Err(NativeClockError::Invalid);
            }
        }
        self.horizons = horizons;
        Ok(())
    }
}

#[cfg(test)]
#[path = "browser_native_clock_fixtures.rs"]
mod tests;
