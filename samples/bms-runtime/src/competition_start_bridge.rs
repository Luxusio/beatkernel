//! Actual native network and system-control adapters for the shared setup gate.
use crate::{
    competition_start_gate::{CompetitionStartPort, CompetitionSetupControl, StartGateResult},
    native_competition_network::NativeCompetitionNetwork,
    native_pump_system::SystemControl,
    multiplayer::{MultiplayerEvent, MultiplayerNotice},
    multiplayer_start::StartSchedule,
};
use std::time::Duration;

impl CompetitionSetupControl for SystemControl {
    fn remaining_duration(deadline: Self::Moment, now: Self::Moment) -> Duration {
        deadline.saturating_duration_since(now)
    }
}

impl CompetitionStartPort for NativeCompetitionNetwork {
    fn try_ready(&mut self) -> StartGateResult<()> {
        Ok(Self::try_ready(self)?)
    }
    fn poll(&mut self) -> StartGateResult<()> {
        for event in Self::poll(self) {
            if let MultiplayerNotice::Session(MultiplayerEvent::Disconnected(error)) = event {
                return Err(error.into());
            }
        }
        Ok(())
    }
    fn start_schedule(&self) -> Option<StartSchedule> {
        Self::start_schedule(self)
    }
    fn release_clock_now_ns(&self) -> StartGateResult<i64> {
        Ok(Self::clock_now_ns(self)?)
    }
    fn max_release_lateness_ns(&self) -> u64 {
        Self::start_policy(self).max_release_lateness_ns
    }
}
