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
    fn config(&self) -> DisciplineConfig {
        DisciplineConfig::default()
    }
    /// Stage a fresh observer on the same output epoch and explicit resumed origins.
    /// Only the new observer may be rebound; refusal leaves this owner unchanged.
    fn restart_for_resume(
        &self,
        output_origin: ClockPoint,
        playback_origin: ClockPoint,
        host_domain: ClockDomainId,
        song_origin: Timestamp,
    ) -> NativeGameplayResult<Self> {
        let mut restarted = Self::new_with_playback_origin(
            self.config(),
            output_origin,
            playback_origin,
            host_domain,
            song_origin,
        )?;
        if let Some(epoch) = self.epoch() {
            match restarted.epoch() {
                Some(current) if current == epoch => {}
                Some(current) if current < epoch => {
                    restarted.rebind_output(epoch, output_origin, playback_origin, song_origin)?
                }
                _ => return Err("resume presentation cannot preserve output epoch".into()),
            }
            if restarted.epoch() != Some(epoch) {
                return Err("resume presentation cannot preserve output epoch".into());
            }
        }
        Ok(restarted)
    }
    /// Stage a strictly newer output epoch without changing this owner or its config.
    fn restart_for_output(
        &self,
        epoch: u64,
        output_origin: ClockPoint,
        playback_origin: ClockPoint,
        host_domain: ClockDomainId,
        song_origin: Timestamp,
    ) -> NativeGameplayResult<Self> {
        let current = self
            .epoch()
            .ok_or("output presentation epoch is unsupported")?;
        if epoch <= current {
            return Err("output presentation epoch must increase".into());
        }
        let mut restarted =
            self.restart_for_resume(output_origin, playback_origin, host_domain, song_origin)?;
        restarted.rebind_output(epoch, output_origin, playback_origin, song_origin)?;
        if restarted.epoch() != Some(epoch) {
            return Err("output presentation cannot preserve requested epoch".into());
        }
        Ok(restarted)
    }
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

/// Cold timing candidates only: no native stream, accepted sample or startup permission.
pub struct PreparedOutputTiming<P: GameplayPresentationPort> {
    pub pause: crate::playback_pause::NativePause,
    pub presentation: P,
    pub basis: beatkernel::audio::OutputFrameBasis,
    pub playback_origin: ClockPoint,
}
/// Stage compatible software clocks while the actual recovered mixer remains held.
/// The caller must pin its producer pause request through native Ready priming.
pub fn prepare_output_timing_rebind<P: GameplayPresentationPort>(
    current: &P,
    pause: &crate::playback_pause::NativePause,
    epoch: u64,
    mixer: &beatkernel::audio::Mixer,
    original_song: Timestamp,
) -> NativeGameplayResult<PreparedOutputTiming<P>> {
    let current_epoch = current
        .epoch()
        .ok_or("output presentation epoch is unsupported")?;
    if current_epoch != pause.epoch() {
        return Err("pause and presentation output epochs differ".into());
    }
    if epoch <= current_epoch {
        return Err("output presentation epoch must increase".into());
    }
    if !mixer.pause_requested() {
        return Err("output timing preparation requires a held producer pause request".into());
    }
    let basis = mixer.output_frame_basis();
    if let Some(pair) = current.latest_pair() {
        if pair.source.domain != basis.origin().domain || pair.target.domain != pause.host_domain()
        {
            return Err("output timing owner clock domains differ".into());
        }
    }
    let mut candidate_pause = pause.clone();
    candidate_pause.rebind_output(epoch, mixer)?;
    let playback_origin = basis.point_at_stream_frame(0)?;
    let song_origin =
        candidate_pause.song_origin_for_presentation(original_song, playback_origin)?;
    let presentation = current.restart_for_output(
        epoch,
        playback_origin,
        playback_origin,
        candidate_pause.host_domain(),
        song_origin,
    )?;
    if presentation.latest_pair().is_some() {
        return Err("prepared output timing must have no accepted observation".into());
    }
    Ok(PreparedOutputTiming {
        pause: candidate_pause,
        presentation,
        basis,
        playback_origin,
    })
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
    fn config(&self) -> DisciplineConfig {
        Self::config(self)
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

/// Cold access to the actual shared producer's exclusive pause lease.
pub struct GameplayPauseControl<'a> {
    owner: PauseControlOwner<'a>,
}
enum PauseControlOwner<'a> {
    Solo(&'a mut crate::local_runtime::SoloRuntime),
    Cohort(&'a mut crate::local_runtime::RuntimeGroup),
}
impl<'a> GameplayPauseControl<'a> {
    pub fn solo(runtime: &'a mut crate::local_runtime::SoloRuntime) -> Self {
        Self {
            owner: PauseControlOwner::Solo(runtime),
        }
    }
    pub fn cohort(group: &'a mut crate::local_runtime::RuntimeGroup) -> Self {
        Self {
            owner: PauseControlOwner::Cohort(group),
        }
    }
    pub fn hold_audio_pause(
        &mut self,
    ) -> Result<beatkernel::audio::PauseHold, beatkernel::audio::PauseHoldError> {
        match &mut self.owner {
            PauseControlOwner::Solo(runtime) => runtime.hold_audio_pause(),
            PauseControlOwner::Cohort(group) => group.hold_audio_pause(),
        }
    }
}
/// Borrowed live timing ownership; publication preserves gameplay/session state.
pub struct GameplayOutputContext<'a, P: GameplayPresentationPort> {
    pub control: GameplayPauseControl<'a>,
    pub presentation: &'a mut P,
    pub pause: &'a mut crate::playback_pause::NativePause,
    pub config: &'a mut crate::native_gameplay::NativeGameplayConfig,
    pub end: &'a mut Option<NativeEnd>,
}
pub trait GameplayDevice {
    type Presentation: GameplayPresentationPort;
    /// No published output clock while a held replacement waits for native evidence.
    fn output_clock_suspended(&self) -> bool {
        false
    }
    /// A held replacement defers coordinated resume until publication or cancellation.
    fn output_replacement_pending(&self) -> bool {
        false
    }
    /// Called only during a committed nonterminal pause; default adapters have no replacement.
    fn publish_paused_output(
        &mut self,
        _: GameplayOutputContext<'_, Self::Presentation>,
    ) -> NativeGameplayResult<bool> {
        Ok(false)
    }
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
    /// Success must leave an accepted latest pair in the staged observer.
    fn seed_resume(
        &mut self,
        discipline: &mut Self::Presentation,
        reference: ClockPair,
    ) -> NativeGameplayResult<()>;
    fn fallback_schedule(&mut self, rate: u32) -> NativeGameplayResult<ClockPoint>;
}

/// A committed paused input cutoff needs host ordering, not extrapolation from
/// a retired output. Callers still validate domains, receipts and chronology.
pub(crate) fn validate_gameplay_host<D: GameplayDevice>(
    device: &D,
    presentation: &D::Presentation,
    point: ClockPoint,
    committed_pause: Option<ClockPoint>,
) -> NativeGameplayResult<()> {
    if device.output_clock_suspended()
        && committed_pause.is_some_and(|boundary| {
            point.domain == boundary.domain && point.timestamp >= boundary.timestamp
        })
    {
        return Ok(());
    }
    presentation.validate_host(point)
}

#[cfg(test)]
#[path = "presentation_rebind_port_fixtures.rs"]
mod presentation_rebind_port_fixtures;

#[cfg(test)]
#[path = "presentation_resume_port_fixtures.rs"]
mod presentation_resume_port_fixtures;

#[cfg(test)]
#[path = "presentation_output_rebind_fixtures.rs"]
mod presentation_output_rebind_fixtures;
