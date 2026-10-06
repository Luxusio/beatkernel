//! Presentation and device contracts owned by gameplay policy.
use crate::{
    live_pause::LivePauseObservation,
    native_end::{EndBoundary, NativeEnd},
    native_gameplay::{InputBatch, NativeGameplayResult},
};
use beatkernel::{
    audio::RenderReport,
    input::PhysicalInputEvent,
    time::{
        ClockDomainId, ClockMappingQuality, ClockPair, ClockPoint, Timestamp,
        presentation::{DisciplineConfig, DisciplineUpdate, PresentationEstimator},
    },
    transport::Transport,
};
use std::collections::VecDeque;

/// Explicit presentation owner; reconstruction preserves playback and song origins.
pub trait GameplayPresentationPort: Sized {
    fn new_with_playback_origin(
        config: DisciplineConfig,
        output_origin: ClockPoint,
        playback_origin: ClockPoint,
        host_domain: ClockDomainId,
        applied_song_origin: Timestamp,
    ) -> NativeGameplayResult<Self>;
    fn epoch(&self) -> Option<u64> {
        None
    }
    fn rebind_output(
        &mut self,
        _epoch: u64,
        _output_origin: ClockPoint,
        _playback_origin: ClockPoint,
        _song_origin: Timestamp,
    ) -> NativeGameplayResult<()> {
        Err("presentation output rebinding is unsupported".into())
    }
    fn latest_pair(&self) -> Option<ClockPair>;
    fn quality(&self) -> ClockMappingQuality;
    fn validate_host(&self, point: ClockPoint) -> NativeGameplayResult<()>;
    fn update(
        &mut self,
        now: ClockPoint,
        transport: &mut Transport,
    ) -> NativeGameplayResult<DisciplineUpdate>;
}

impl GameplayPresentationPort for PresentationEstimator {
    fn new_with_playback_origin(
        config: DisciplineConfig,
        output_origin: ClockPoint,
        playback_origin: ClockPoint,
        host_domain: ClockDomainId,
        applied_song_origin: Timestamp,
    ) -> NativeGameplayResult<Self> {
        Ok(Self::new_with_playback_origin(
            config,
            output_origin,
            playback_origin,
            host_domain,
            applied_song_origin,
        )?)
    }
    fn epoch(&self) -> Option<u64> {
        Some(Self::epoch(self))
    }
    fn rebind_output(
        &mut self,
        epoch: u64,
        output_origin: ClockPoint,
        playback_origin: ClockPoint,
        song_origin: Timestamp,
    ) -> NativeGameplayResult<()> {
        Ok(Self::rebind_output(
            self,
            epoch,
            output_origin,
            playback_origin,
            song_origin,
        )?)
    }
    fn latest_pair(&self) -> Option<ClockPair> {
        Self::latest_pair(self)
    }
    fn quality(&self) -> ClockMappingQuality {
        Self::quality(self)
    }
    fn validate_host(&self, point: ClockPoint) -> NativeGameplayResult<()> {
        Ok(Self::validate_host(self, point)?)
    }
    fn update(
        &mut self,
        now: ClockPoint,
        transport: &mut Transport,
    ) -> NativeGameplayResult<DisciplineUpdate> {
        Ok(Self::update(self, now, transport)?)
    }
}

pub trait GameplayDevice {
    type Presentation: GameplayPresentationPort;
    fn observe(&mut self, discipline: &mut Self::Presentation) -> NativeGameplayResult<()>;
    /// Interval owners override this with original coherent evidence;
    /// correction-only midpoint pairs cannot establish their pause boundary.
    fn pause_observation(
        &mut self,
        reference: ClockPair,
    ) -> NativeGameplayResult<LivePauseObservation> {
        Ok(LivePauseObservation::Point(reference))
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>>;
    fn host_now(&self) -> NativeGameplayResult<ClockPoint>;
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch>;
    fn observe_end(
        &mut self,
        end: &mut NativeEnd,
        discipline: &Self::Presentation,
        report: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>>;
    /// Reseed from the device owner's original observation evidence.
    fn seed_resume(
        &mut self,
        discipline: &mut Self::Presentation,
        reference: ClockPair,
    ) -> NativeGameplayResult<()>;
    fn fallback_schedule(&mut self, rate: u32) -> NativeGameplayResult<ClockPoint>;
}

#[cfg(test)]
#[path = "presentation_rebind_port_fixtures.rs"]
mod presentation_rebind_port_fixtures;
