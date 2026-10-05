//! Exact publication cadence with an injected clock, outside gameplay clocks.
use crate::{competition_progress::CompetitionProgressPort, multiplayer_group::MemberProgress};

pub const PUBLICATION_INTERVAL_NS: u64 = 50_000_000;
pub trait CompetitionProgressClock {
    type Error;
    fn now_ns(&mut self) -> Result<u64, Self::Error>;
}
pub enum CadenceError<P, C> {
    Publication(P),
    Clock(C),
    ClockRegressed,
}
#[derive(Default)]
pub struct ProgressCadence {
    last_observed: Option<u64>,
    last_published: Option<u64>,
}
impl ProgressCadence {
    pub const fn new() -> Self {
        Self {
            last_observed: None,
            last_published: None,
        }
    }
    pub const fn last_observed(&self) -> Option<u64> {
        self.last_observed
    }
    pub const fn last_published(&self) -> Option<u64> {
        self.last_published
    }
    fn observe(&mut self, now: u64) -> bool {
        if self.last_observed.is_some_and(|previous| now < previous) {
            return false;
        }
        self.last_observed = Some(now);
        true
    }
}
/// A successful effect with a failed post-effect sample cannot be rolled back.
/// The marker stays unchanged in that case; the owner must disable comparison before retry.
pub fn publish_progress_with_clock<P: CompetitionProgressPort, C: CompetitionProgressClock>(
    port: &mut P,
    clock: &mut C,
    cadence: &mut ProgressCadence,
    members: &[MemberProgress],
    allowed: bool,
    require_start: bool,
) -> Result<bool, CadenceError<P::Error, C::Error>> {
    if !allowed || !port.ready() || (require_start && !port.started()) {
        return Ok(false);
    }
    let now = clock.now_ns().map_err(CadenceError::Clock)?;
    if !cadence.observe(now) {
        return Err(CadenceError::ClockRegressed);
    }
    if cadence
        .last_published
        .is_some_and(|last| now - last < PUBLICATION_INTERVAL_NS)
    {
        return Ok(false);
    }
    port.publish(members).map_err(CadenceError::Publication)?;
    let completed = clock.now_ns().map_err(CadenceError::Clock)?;
    if !cadence.observe(completed) {
        return Err(CadenceError::ClockRegressed);
    }
    cadence.last_published = Some(completed);
    Ok(true)
}
#[cfg(test)]
#[path = "competition_progress_cadence_fixtures.rs"]
mod fixtures;
