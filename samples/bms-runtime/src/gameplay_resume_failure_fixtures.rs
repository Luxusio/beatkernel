//! Shared fault injection around the real memory device; no alternate pump.
pub(crate) use crate::gameplay_presentation_port_fixtures::*;
use crate::live_pause::LivePauseBoundary;
#[derive(Clone, Copy)]
pub(crate) enum SeedMode {
    FailAfterAdmission,
    NoObservation,
}
#[derive(Debug)]
pub(crate) struct SeedRefusal(u64);
impl std::fmt::Display for SeedRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original seed refusal")
    }
}
impl std::error::Error for SeedRefusal {}
pub(crate) struct FaultDevice {
    pub inner: MemoryDevice,
    pub mode: SeedMode,
    pub original_address: usize,
    failure: Option<Box<SeedRefusal>>,
    pub seed_calls: usize,
    pub before_seed: Option<PresentationEstimator>,
    pub temporary_pair: Option<ClockPair>,
}
impl FaultDevice {
    pub fn new(inner: MemoryDevice, mode: SeedMode) -> Self {
        let error = Box::new(SeedRefusal(0xcafe));
        let original_address = error.as_ref() as *const SeedRefusal as usize;
        Self {
            inner,
            mode,
            original_address,
            failure: Some(error),
            seed_calls: 0,
            before_seed: None,
            temporary_pair: None,
        }
    }
}
impl GameplayDevice for FaultDevice {
    type Presentation = PresentationEstimator;
    fn observe(&mut self, p: &mut Self::Presentation) -> NativeGameplayResult<()> {
        self.inner.observe(p)?;
        self.before_seed = Some(p.clone());
        Ok(())
    }
    fn pause_observation(
        &mut self,
        r: ClockPair,
    ) -> NativeGameplayResult<crate::live_pause::LivePauseObservation> {
        self.inner.pause_observation(r)
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        self.inner.render_report()
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        self.inner.host_now()
    }
    fn acquire(
        &mut self,
        e: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        self.inner.acquire(e)
    }
    fn observe_end(
        &mut self,
        e: &mut NativeEnd,
        p: &Self::Presentation,
        r: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        self.inner.observe_end(e, p, r)
    }
    fn seed_resume(
        &mut self,
        p: &mut Self::Presentation,
        r: ClockPair,
    ) -> NativeGameplayResult<()> {
        self.seed_calls += 1;
        match self.mode {
            SeedMode::FailAfterAdmission => {
                self.inner.seed_resume(p, r)?;
                self.temporary_pair = p.latest_pair();
                Err(self.failure.take().unwrap())
            }
            SeedMode::NoObservation => {
                assert!(p.latest_pair().is_none());
                Ok(())
            }
        }
    }
    fn fallback_schedule(&mut self, r: u32) -> NativeGameplayResult<ClockPoint> {
        self.inner.fallback_schedule(r)
    }
}
pub(crate) struct TraceHost {
    pub inner: Host,
    pub boundaries: Vec<LivePauseBoundary>,
}
impl NativeGameplayHost for TraceHost {
    fn cancelled(&self) -> bool {
        self.inner.cancelled()
    }
    fn pause_requested(&self) -> bool {
        self.inner.pause_requested()
    }
    fn retry_pause_publication(&mut self) {
        self.inner.retry_pause_publication()
    }
    fn publish_pause(&mut self, p: PauseState) {
        self.inner.publish_pause(p)
    }
    fn publish_section_end(&mut self, e: Timestamp) {
        self.inner.publish_section_end(e)
    }
    fn publish_report(&mut self, r: &RuntimeReport) -> NativeGameplayResult<()> {
        self.inner.publish_report(r)
    }
    fn publish_local_reports(
        &mut self,
        r: &[crate::local_runtime::PlayerReport],
    ) -> NativeGameplayResult<()> {
        self.inner.publish_local_reports(r)
    }
    fn diagnostic(&mut self, d: NativeGameplayDiagnostic<'_>) {
        if let NativeGameplayDiagnostic::Pause { boundary, .. } = d {
            self.boundaries.push(boundary);
        }
        self.inner.diagnostic(d);
    }
}
pub(crate) fn settings() -> DisciplineConfig {
    DisciplineConfig {
        capacity: 16,
        retention_interval: Duration::from_nanos(10_000_000),
        min_span: Duration::from_nanos(100_000_000),
        correction_horizon: Duration::from_nanos(9_000_000_000),
        ..Default::default()
    }
}
pub(crate) fn limits() -> beatkernel::replay::codec::ReplayCodecLimits {
    beatkernel::replay::codec::ReplayCodecLimits::new(
        65536,
        128,
        4096,
        beatkernel::input::CodecLimits::new(4096, 4096).unwrap(),
    )
    .unwrap()
}
pub(crate) fn assert_original_error(
    mode: SeedMode,
    error: &(dyn std::error::Error + 'static),
    device: &FaultDevice,
) {
    match mode {
        SeedMode::FailAfterAdmission => {
            let typed = error.downcast_ref::<SeedRefusal>().unwrap();
            assert_eq!(typed.0, 0xcafe);
            assert_eq!(
                typed as *const SeedRefusal as usize,
                device.original_address
            );
            assert!(device.temporary_pair.is_some());
        }
        SeedMode::NoObservation => {
            assert_eq!(
                error.to_string(),
                "resume presentation seed has no accepted observation"
            );
            assert_eq!(device.temporary_pair, None);
        }
    }
    assert_eq!(device.seed_calls, 1);
}
pub(crate) fn assert_paused_history(transport: &Transport, host: &TraceHost) {
    let paused = host.boundaries.iter().find(|b| b.paused).unwrap();
    assert!(host.boundaries.iter().any(|b| !b.paused));
    let mut expected = Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL);
    expected.pause(paused.at.timestamp).unwrap();
    expected.seek(paused.at.timestamp, paused.song).unwrap();
    assert_eq!(transport, &expected);
    assert!(transport.is_paused());
    assert_eq!(transport.anchors(), expected.anchors());
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(1_000_000_000))
            .unwrap(),
        paused.song
    );
}
pub(crate) fn assert_old_presentation(
    actual: &PresentationEstimator,
    device: &FaultDevice,
    epoch: u64,
) {
    let old = device.before_seed.as_ref().unwrap();
    assert!(old.retained_len() > 1);
    assert_eq!(actual.epoch(), epoch);
    assert_eq!(actual.config(), settings());
    assert_eq!(actual.latest_pair(), old.latest_pair());
    assert_eq!(actual.retained_len(), old.retained_len());
}
pub(crate) fn assert_capture_prefix(
    capture: &crate::replay_capture::LiveReplayCapture,
    host: &TraceHost,
) {
    let paused = host.boundaries.iter().find(|b| b.paused).unwrap();
    assert!(!capture.records().is_empty());
    for record in capture.records() {
        assert!(record.song_time <= paused.song);
        if let beatkernel::replay::ReplayOperation::Input(input) = &record.operation {
            if let PhysicalInputEvent::Button(button) = &input.physical {
                assert_ne!(button.state, ButtonState::Up);
            }
        }
    }
    crate::replay_playback::reconstruct(
        &source(),
        beatkernel::replay::codec::ReplayFile::new(
            capture.header().clone(),
            capture.records().to_vec(),
        ),
        limits(),
    )
    .unwrap();
}
