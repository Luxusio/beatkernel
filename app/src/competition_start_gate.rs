//! Deterministic setup waiting; control time never substitutes network release time.
use crate::{
    native_gameplay::NativeGameplayResult, native_pump_control::NativePumpControl,
    multiplayer::MultiplayerError, multiplayer_start::StartSchedule,
};
use std::time::Duration;

pub type StartGateResult<T> = NativeGameplayResult<T>;
type Result<T> = StartGateResult<T>;

pub trait CompetitionStartPort {
    fn try_ready(&mut self) -> StartGateResult<()>;
    fn poll(&mut self) -> StartGateResult<()>;
    fn start_schedule(&self) -> Option<StartSchedule>;
    fn release_clock_now_ns(&self) -> StartGateResult<i64>;
    fn max_release_lateness_ns(&self) -> u64;
}

pub trait CompetitionSetupControl: NativePumpControl {
    fn remaining_duration(deadline: Self::Moment, now: Self::Moment) -> Duration;
}

/// Deadline admission precedes readiness. Every effect retains original errors.
pub fn await_start<N: CompetitionStartPort, C: CompetitionSetupControl>(
    network: &mut N,
    control: &mut C,
    timeout: Duration,
    await_release: bool,
    mut service: impl FnMut() -> StartGateResult<bool>,
) -> StartGateResult<bool> {
    if timeout.is_zero() {
        return Err(MultiplayerError::SetupTimeout.into());
    }
    let mut last = control.now()?;
    let deadline = C::checked_add(last, timeout).ok_or("competition setup deadline overflow")?;
    if deadline <= last {
        return Err("competition setup deadline did not advance".into());
    }
    network.try_ready()?;
    loop {
        if !service()? {
            return Ok(false);
        }
        network.poll()?;
        let now = control.now()?;
        if now < last {
            return Err("competition setup control clock regressed".into());
        }
        last = now;
        if now >= deadline {
            return Err(MultiplayerError::SetupTimeout.into());
        }
        if let Some(schedule) = network.start_schedule() {
            if !await_release {
                return Ok(true);
            }
            if start_release_due(
                schedule,
                network.release_clock_now_ns()?,
                network.max_release_lateness_ns(),
            )? {
                return Ok(true);
            }
        }
        let now = control.now()?;
        if now < last {
            return Err("competition setup control clock regressed".into());
        }
        last = now;
        control.wait(Duration::from_millis(5).min(C::remaining_duration(deadline, now)))?;
    }
}

pub fn start_release_due(
    schedule: crate::multiplayer_start::StartSchedule,
    now: i64,
    max_lateness_ns: u64,
) -> Result<bool> {
    if now < 0 || schedule.target_ns < 0 {
        return Err(crate::multiplayer::MultiplayerError::Protocol(
            "negative committed start timestamp".into(),
        )
        .into());
    }
    if now < schedule.target_ns {
        return Ok(false);
    }
    if (now - schedule.target_ns) as u64 > max_lateness_ns {
        return Err(crate::multiplayer::MultiplayerError::Protocol(
            "committed software start release exceeded lateness policy".into(),
        )
        .into());
    }
    Ok(true)
}
