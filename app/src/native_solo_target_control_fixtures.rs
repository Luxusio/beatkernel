//! Real converted output and the existing solo Runtime; native evidence is DI only.
use super::*;
use crate::{
    gameplay::output::ports::{
        NativeTargetPlaybackFacts, NativeTargetPlaybackObservation, NativeTargetPlaybackQuery,
    },
    native_audio_presentation::TargetNativeAudioSnapshot,
    native_coordinate_control::{CoordinatePlaybackControlFacts, TargetCoordinateControlFacts},
    native_output_coordinate::CoordinateTargetNativeAudioSnapshot,
};
use beatkernel::audio::{ChannelMatrix, ConvertedRenderReport, ResampleQuality};
use beatkernel_platform::audio::{ConvertedNativeOutputState, DeviceFormat, SampleEncoding};

fn exact(time: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap()
}
fn target_original(basis: TargetFrameBasis, frame: u64) -> CoordinateTargetNativeAudioSnapshot {
    CoordinateTargetNativeAudioSnapshot::new(
        TargetNativeAudioSnapshot {
            epoch: 0,
            basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(frame).unwrap(),
                target: cp(1, 100 + (frame * 1_000_000_000 / 7) as i64),
            }),
        },
        NativeOutputCoordinate::StreamFrame { frame },
    )
    .unwrap()
}

struct RetirementCall {
    step: usize,
    query: InterpolatedTargetTime,
    future_input: InterpolatedTargetTime,
    identity: SourcePlaybackIdentity,
    source_minimum: Option<u64>,
    acquire_calls: usize,
    committed_input: Option<ClockPoint>,
}

struct TargetDevice {
    owner: ConvertedNativeOutputState,
    report: ConvertedRenderReport,
    basis: TargetFrameBasis,
    shared: Rc<Cell<usize>>,
    step: usize,
    stop_at: usize,
    missing_steps: Vec<usize>,
    report_missing_steps: Vec<usize>,
    stale_step: Option<usize>,
    last_facts: Option<CoordinatePlaybackControlFacts>,
    original_events: VecDeque<PhysicalInputEvent>,
    acquired_events: usize,
    acquired: Vec<(
        usize,
        Option<RenderReport>,
        beatkernel_platform::audio::ConvertedBoundaryFacts,
    )>,
    held: bool,
    hold_calls: Vec<(usize, bool)>,
    held_callbacks: Vec<(
        usize,
        u64,
        u64,
        beatkernel::audio::SourcePosition,
        beatkernel::audio::SourcePosition,
    )>,
    retirement_calls: Vec<RetirementCall>,
    retirement_busy: bool,
    retirement_error_step: Option<usize>,
    acquire_calls: usize,
    backlog_steps: Vec<usize>,
}
impl TargetDevice {
    fn frame(&self) -> u64 {
        self.report.target_frame_cursor
    }
    fn pair(&self) -> ClockPair {
        match target_original(self.basis, self.frame())
            .snapshot()
            .evidence
        {
            OriginalNativePresentationEvidence::SuppliedPair(pair) => pair,
            _ => unreachable!(),
        }
    }
    fn packet(
        &self,
        query: InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<NativeTargetPlaybackQuery<()>> {
        let history = self.owner.phase_history().unwrap();
        let mut selected = None;
        for i in 0..history.len() {
            let phase = history.span(i).unwrap();
            let start = phase.to_words();
            let start = TargetTime::new(start[4], start[5], start[6]).unwrap();
            if query.compare_target_time(start).is_lt() {
                continue;
            }
            let end = history
                .span(i + 1)
                .map(|next| {
                    let words = next.to_words();
                    TargetTime::new(words[4], words[5], words[6]).unwrap()
                })
                .unwrap_or(history.end_time());
            if query.compare_target_time(end).is_gt()
                || (i + 1 < history.len() && query.compare_target_time(end).is_eq())
            {
                continue;
            }
            selected = Some((phase, end));
        }
        let (phase, segment_end) = selected.ok_or_else(|| {
            std::io::Error::other("exact Target query outside actual converted history")
        })?;
        let source = self
            .owner
            .converter_owner()
            .source_playback_history()
            .unwrap();
        let playback = (0..source.len())
            .filter_map(|i| source.span(i))
            .find(|span| {
                phase
                    .compare_source_frame_at_interpolated_time(
                        query,
                        segment_end,
                        span.physical_start(),
                    )
                    .is_ok_and(|order| !order.is_lt())
                    && span.physical_end().is_none_or(|end| {
                        phase
                            .compare_source_frame_at_interpolated_time(query, segment_end, end)
                            .is_ok_and(|order| order.is_lt())
                    })
            })
            .ok_or_else(|| {
                std::io::Error::other("exact converted query outside actual source span")
            })?;
        Ok(NativeTargetPlaybackQuery::new(
            0,
            self.basis,
            identity,
            query,
            NativeTargetPlaybackFacts {
                playback: ConvertedPlaybackFacts {
                    phase,
                    segment_end,
                    generated_target_end: history.end_time(),
                    submitted_target_end: self.owner.admitted_target_time().unwrap(),
                    playback,
                    pulled_source_end: source.pulled_end_frame(),
                    revision: (self.step as u64 + 1) * 2,
                },
                source: if self.report_missing_steps.contains(&self.step) {
                    None
                } else {
                    self.owner.last_real_source_report()
                },
                converted: Some(self.report),
                boundaries: self.owner.boundaries(),
                revision: (self.step as u64 + 1) * 4,
            },
            Some(()),
        )?)
    }
}
impl GameplayDevice for TargetDevice {
    type Presentation = PresentationEstimator;
    fn request_history_retirement(
        &mut self,
        presentation: &NativeAudioPresentation,
        minimum_query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
        source_minimum: Option<u64>,
    ) -> NativeGameplayResult<Option<u64>> {
        // The DI effect boundary checks the actual owner and current descriptor.
        // A recorded queue revision never stands in for a native pause ACK.
        assert_eq!(presentation.authority().epoch().id, 0);
        assert_eq!(
            presentation.authority().coordinate_basis(),
            Some(AudioAuthorityCoordinateBasis::TargetFrame(self.basis))
        );
        let source = self
            .owner
            .converter_owner()
            .source_playback_history()
            .unwrap();
        assert_eq!(identity.hz(), 3);
        assert_eq!(identity.initial_physical(), source.initial_physical_frame());
        assert_eq!(identity.initial_playback(), source.initial_playback_frame());
        let packet = self.packet(*minimum_query, identity)?;
        assert!(minimum_query
            .compare_target_time(packet.facts().playback.submitted_target_end)
            .is_le());
        self.retirement_calls.push(RetirementCall {
            step: self.step,
            query: *minimum_query,
            future_input: presentation.authority().future_input_retention_query()?,
            identity,
            source_minimum,
            acquire_calls: self.acquire_calls,
            committed_input: presentation.authority().committed_input_host(),
        });
        if self.retirement_error_step == Some(self.step) {
            return Err("scripted retirement dispatch refusal".into());
        }
        Ok((!self.retirement_busy).then_some(2))
    }
    fn observe_coordinate_playback_control(
        &mut self,
        presentation: &mut NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinatePlaybackControlFacts>> {
        self.step += 1;
        self.shared.set(self.step);
        if self.held {
            let physical = self.owner.mixer().frame_cursor();
            let position = self.report.source_position;
            self.report = self.owner.render_held_pending(7)?;
            self.held_callbacks.push((
                self.step,
                physical,
                self.owner.mixer().frame_cursor(),
                position,
                self.report.source_position,
            ));
        } else {
            self.report = self.owner.render_pending(7)?;
        }
        self.owner.admit(7)?;
        self.acquired.push((
            self.step,
            self.owner.last_real_source_report(),
            self.owner.boundaries(),
        ));
        if self.missing_steps.contains(&self.step) {
            return Ok(None);
        }
        let coordinate = target_original(self.basis, self.frame());
        let query = exact(
            self.basis
                .start_time()
                .checked_add_frames(self.frame(), 7)?,
        );
        let packet = self.packet(query, identity)?;
        let observation = NativeTargetPlaybackObservation::new(coordinate, packet)?;
        presentation.admit_target_coordinate(coordinate)?;
        let facts = CoordinatePlaybackControlFacts::Target(
            TargetCoordinateControlFacts::from_target(&observation, presentation, identity)?,
        );
        if self.stale_step == Some(self.step) {
            return Ok(self.last_facts);
        }
        self.last_facts = Some(facts);
        Ok(Some(facts))
    }
    fn observe_coordinate_control(
        &mut self,
        _: &mut NativeAudioPresentation,
        _: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinateControlFacts>> {
        panic!("Target must use common compound port")
    }
    fn observe(&mut self, _: &mut Self::Presentation) -> NativeGameplayResult<()> {
        panic!("no scalar observer")
    }
    fn observe_audio(&mut self, _: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
        panic!("no independent audio observation")
    }
    // The existing stop barrier reads real latest render evidence after queued
    // admissions; this never supplies a missing compound control publication.
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        Ok(self.owner.last_real_source_report())
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(self.pair().target)
    }
    fn set_audio_held(&mut self, held: bool) -> NativeGameplayResult<()> {
        self.hold_calls.push((self.step, held));
        self.held = held;
        Ok(())
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        self.acquire_calls += 1;
        while self
            .original_events
            .front()
            .is_some_and(|event| event.meta().timestamp <= self.pair().target.timestamp)
        {
            events.push_back(self.original_events.pop_front().unwrap());
            self.acquired_events += 1;
        }
        Ok(InputBatch {
            completed_through: (!self.backlog_steps.contains(&self.step))
                .then_some(self.pair().target),
            backlog: self.backlog_steps.contains(&self.step),
            closed: self.step >= self.stop_at,
        })
    }
    fn observe_end(
        &mut self,
        _: &mut NativeEnd,
        _: &Self::Presentation,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        panic!("no scalar end")
    }
    fn seed_resume(
        &mut self,
        _: &mut Self::Presentation,
        _: ClockPair,
    ) -> NativeGameplayResult<()> {
        panic!("no scalar resume")
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        panic!("no scalar schedule")
    }
    fn playback_mapping_at(
        &self,
        _: &NativeAudioPresentation,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<NativePlaybackMapping>> {
        Ok(Some(NativePlaybackMapping::from_target(
            &self.packet(*query, identity)?,
        )))
    }
}

struct TargetFixture {
    device: TargetDevice,
    runtime: SoloRuntime,
    presentation: NativeAudioPresentation,
    gauge: BmsGauge,
    bgm: BgmFeeder,
    pause: NativePause,
    end: Option<NativeEnd>,
    completion: Option<SongCompletion>,
    capture: Option<LiveReplayCapture>,
    competition: Option<NoopSoloCompetition>,
    delivery: InputDeliveryTelemetry,
    pre: u64,
    merger: InputMerger,
    cfg: CoordinateGameplayConfig,
    speed: LiveSpeedSetup,
    host: Host,
}
impl TargetFixture {
    fn new(rate: Rate, end_song: Option<Timestamp>) -> Self {
        // Reuse the actual Runtime's producer and its matching Mixer consumer.
        // No new simulation, command queue, copied Source control oracle or fake report.
        let Fixture {
            device,
            runtime,
            gauge,
            bgm,
            pause,
            end,
            completion,
            capture,
            competition,
            delivery,
            pre,
            merger,
            mut cfg,
            speed,
            host,
            ..
        } = Fixture::configured(false, rate, Duration::ZERO, end_song);
        let shared = device.shared.clone();
        let mut owner = ConvertedNativeOutputState::new(
            device.mixer,
            DeviceFormat::new(7, 1, SampleEncoding::Float32, None).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            ResampleQuality::Linear,
            16,
        )
        .unwrap_or_else(|_| panic!("valid actual 3 to 7 converter"));
        owner.enable_phase_history(8).unwrap();
        let basis = owner.target_frame_basis();
        assert_eq!(basis.start_time(), TargetTime::from_frames(4, 3).unwrap());
        let report = owner.render_pending(7).unwrap();
        owner.admit(7).unwrap();
        let mut presentation = new_coordinate_audio_presentation(
            0,
            AudioAuthorityCoordinateBasis::TargetFrame(basis),
            ClockDomainId(1),
            cp(3, 500),
            AudioAuthorityConfig::default(),
        )
        .unwrap();
        for frame in [0, 2] {
            presentation
                .admit_target_coordinate(target_original(basis, frame))
                .unwrap();
        }
        cfg.audio.gameplay.stream_origin = presentation.authority().epoch().stream_origin;
        cfg.audio.gameplay.playback_origin = cfg.audio.gameplay.stream_origin;
        // Schedule commands on the actual source report's playback grid; this
        // is independent of the Target-native authority used for judgement.
        cfg.audio.gameplay.logical_schedule = true;
        Self {
            device: TargetDevice {
                owner,
                report,
                basis,
                shared,
                step: 0,
                stop_at: 8,
                missing_steps: vec![],
                report_missing_steps: vec![],
                stale_step: None,
                last_facts: None,
                original_events: VecDeque::new(),
                acquired_events: 0,
                acquired: vec![],
                held: false,
                hold_calls: vec![],
                held_callbacks: vec![],
                retirement_calls: vec![],
                retirement_busy: false,
                retirement_error_step: None,
                acquire_calls: 0,
                backlog_steps: vec![],
            },
            runtime,
            presentation,
            gauge,
            bgm,
            pause: pause.with_target_basis(0, basis).unwrap(),
            end: end.map(|end| end.with_target_basis(0, basis).unwrap()),
            completion,
            capture,
            competition,
            delivery,
            pre,
            merger,
            cfg,
            speed,
            host,
        }
    }
    fn run(&mut self) -> NativeGameplayResult<Option<CompletedPlayResult>> {
        run_gameplay_audio_coordinate_with_result_and_ports(
            &mut self.device,
            AudioGameplaySession {
                session: GameplaySession {
                    runtime: &mut self.runtime,
                    gauge: &mut self.gauge,
                    bgm: &mut self.bgm,
                    discipline: &mut self.presentation,
                    pause: &mut self.pause,
                    end: &mut self.end,
                    completion: &mut self.completion,
                    capture: &mut self.capture,
                    competition: &mut self.competition,
                    delivery: &mut self.delivery,
                    pre_origin_inputs: &mut self.pre,
                },
                merger: &mut self.merger,
            },
            self.cfg,
            &mut Control::default(),
            &mut self.host,
            Some(self.speed),
        )
    }
}

#[test]
fn target_actual_entry_pause_resume_uses_actual_projected_boundaries_and_frozen_capture_for_four_rates(
) {
    for rate in [
        Rate::NORMAL,
        Rate::new(1, 2).unwrap(),
        Rate::new(3, 2).unwrap(),
        Rate::new(2, 1).unwrap(),
    ] {
        let mut f = TargetFixture::new(rate, None);
        assert!(f.run().unwrap().is_none());
        assert_eq!(f.pause.phase(), PausePhase::Running);
        assert!(!f.runtime.transport_mut().is_paused());
        assert!(f.host.pauses.contains(&PauseState::Paused));
        assert!(f.host.pauses.contains(&PauseState::Resuming));
        let (_, pause_report, boundaries) = f
            .device
            .acquired
            .iter()
            .find(|(_, _, boundaries)| boundaries.pause.is_some())
            .unwrap();
        let cut = boundaries.pause.unwrap();
        let report = pause_report.unwrap();
        assert!(report.paused);
        assert_eq!(cut.source_frame, report.start_frame);
        let elapsed = ((report.playback_start_frame - 1) as i128 * 1_000_000_000 / 3) as i64;
        let frozen = song_at(elapsed, 0, rate);
        let pause_step = f
            .host
            .report_steps
            .iter()
            .zip(&f.host.reports)
            .find(|(_, report)| report.song_time == frozen)
            .unwrap()
            .0;
        assert!(*pause_step >= 2);
        assert!(f
            .host
            .reports
            .iter()
            .zip(&f.host.report_steps)
            .any(|(r, s)| *s >= 5 && r.song_time == frozen));
        assert!(f
            .capture
            .as_ref()
            .unwrap()
            .records()
            .iter()
            .any(|r| r.song_time == frozen));
        assert!(f
            .device
            .acquired
            .iter()
            .any(|(_, report, boundaries)| boundaries.resume.is_some()
                && report.is_some_and(|r| !r.paused)));
        assert_eq!(
            f.runtime
                .transport_mut()
                .output_phase_basis()
                .unwrap()
                .source_hz(),
            3
        );
        assert_eq!(f.device.basis.sample_rate(), 7);
        assert_eq!(
            f.cfg.source_history,
            SourcePlaybackIdentity::new(3, 0, 0).unwrap()
        );
    }
}

#[test]
fn target_missing_pause_packet_never_acknowledges_native_request() {
    let mut f = TargetFixture::new(Rate::NORMAL, None);
    f.device.missing_steps = vec![2, 3, 4];
    f.device.stop_at = 4;
    // Each generated seven-frame block advances the original native HOST by
    // one second. Losing three publications must hit the real default age
    // guard rather than extrapolate or grant a pause acknowledgement.
    assert!(f
        .run()
        .unwrap_err()
        .to_string()
        .contains("declared age bound"));
    assert_eq!(f.pause.phase(), PausePhase::Pausing);
    assert!(f.device.owner.mixer().pause_requested());
    assert!(!f.runtime.transport_mut().is_paused());
    assert!(!f.host.pauses.contains(&PauseState::Paused));
    assert!(f.host.report_steps.iter().all(|s| *s == 1));
    assert!(f.device.hold_calls.is_empty());
    assert!(!f.device.held);
}

#[test]
fn target_stale_original_packet_refuses_before_runtime_pause_ack() {
    let mut f = TargetFixture::new(Rate::NORMAL, None);
    f.device.stale_step = Some(2);
    assert!(f.run().is_err());
    assert_eq!(f.pause.phase(), PausePhase::Pausing);
    assert!(!f.runtime.transport_mut().is_paused());
    assert!(!f.host.pauses.contains(&PauseState::Paused));
    assert!(f.host.report_steps.iter().all(|s| *s == 1));
    assert!(f.device.hold_calls.is_empty());
    assert!(!f.device.held);
}

#[test]
fn target_actual_finite_endpoint_completes_from_real_end_projection() {
    let mut f = TargetFixture::new(Rate::NORMAL, Some(Timestamp::from_nanos(2_000_000_000)));
    f.host.pause_enabled = false;
    assert!(f.run().unwrap().is_some());
    let end = f.device.owner.boundaries().end.unwrap();
    assert_eq!(end.source_frame, 7);
    assert_eq!(
        f.runtime.judge().effective_song_time(),
        Some(Timestamp::from_nanos(2_000_000_000))
    );
    assert_eq!(
        f.host.reports.last().unwrap().song_time,
        Timestamp::from_nanos(2_000_000_000)
    );
    assert!(f
        .capture
        .as_ref()
        .unwrap()
        .records()
        .iter()
        .any(|r| r.song_time == Timestamp::from_nanos(2_000_000_000)));
    assert!(f
        .device
        .owner
        .last_real_source_report()
        .unwrap()
        .playback_end_physical_frame
        .is_some());
}

#[test]
fn target_genuine_pause_ack_capture_survives_post_operation_publication_failure() {
    let mut f = TargetFixture::new(Rate::NORMAL, None);
    f.host.fail_step = Some(2);
    assert!(f
        .run()
        .unwrap_err()
        .downcast_ref::<NativeReportObservationError>()
        .is_some());
    assert_eq!(f.pause.phase(), PausePhase::Paused);
    assert!(f.runtime.transport_mut().is_paused());
    assert!(f.presentation.authority().committed_operation().is_some());
    assert_eq!(
        f.host.reports.last().unwrap().song_time,
        f.capture
            .as_ref()
            .unwrap()
            .records()
            .last()
            .unwrap()
            .song_time
    );
    assert_eq!(f.device.hold_calls, vec![(2, true)]);
    assert!(f.device.held);
}

#[test]
fn target_missing_report_keeps_native_pause_pending_and_original_later_input_unjudged() {
    let mut f = TargetFixture::new(Rate::NORMAL, None);
    f.device.report_missing_steps = vec![2, 3, 4];
    f.device.stop_at = 4;
    f.device
        .original_events
        .push_back(input(3_000_000_100, 0, ButtonState::Down));
    assert!(f.run().unwrap().is_none());
    assert_eq!(f.pause.phase(), PausePhase::Pausing);
    assert!(!f.runtime.transport_mut().is_paused());
    assert!(!f.host.pauses.contains(&PauseState::Paused));
    assert!(f.host.reports.iter().all(|r| r.input.is_none()));
    assert_eq!(f.presentation.authority().committed_input_host(), None);
}

#[test]
fn target_stale_real_finite_marker_refuses_before_runtime_is_sealed() {
    let mut f = TargetFixture::new(Rate::NORMAL, Some(Timestamp::from_nanos(2_000_000_000)));
    f.host.pause_enabled = false;
    let coordinate = target_original(f.device.basis, 2);
    let query = exact(
        f.device
            .basis
            .start_time()
            .checked_add_frames(2, 7)
            .unwrap(),
    );
    let packet = f.device.packet(query, f.cfg.source_history).unwrap();
    assert_eq!(packet.facts().boundaries.end.unwrap().source_frame, 7);
    assert!(packet
        .facts()
        .source
        .unwrap()
        .playback_end_physical_frame
        .is_some());
    let observation = NativeTargetPlaybackObservation::new(coordinate, packet).unwrap();
    let facts = TargetCoordinateControlFacts::from_target(
        &observation,
        &f.presentation,
        f.cfg.source_history,
    )
    .unwrap();
    f.device.last_facts = Some(facts.into());
    f.device.stale_step = Some(1);
    assert!(f.runtime.gameplay_fence().is_none());
    assert!(f.run().is_err());
    assert!(f.runtime.gameplay_fence().is_none());
    assert!(f.host.reports.is_empty());
    assert!(f.capture.as_ref().unwrap().records().is_empty());
    assert!(f.presentation.authority().committed_operation().is_none());
}

#[test]
fn target_cold_invalid_capture_leaves_real_native_owners_and_mixer_unchanged() {
    let mut f = TargetFixture::new(Rate::new(2, 1).unwrap(), None);
    let policy = ResolvedPlayPolicy::builtin(0, 0, 0).unwrap();
    f.capture = Some(
        LiveReplayCapture::new_with_policy_and_speed(
            f.runtime.judge(),
            ClockDomainId(3),
            limits(),
            Timestamp::ZERO,
            0,
            None,
            beatkernel_bms::BmsInputMode::ButtonOnly,
            None,
            &policy,
            ReplaySpeed::new(Rate::NORMAL).unwrap(),
        )
        .unwrap(),
    );
    let before_pause = f.pause.clone();
    let before_presentation = format!("{:?}", f.presentation);
    let physical = f.device.owner.mixer().frame_cursor();
    let transport = f.runtime.transport_mut().clone();
    assert!(f.run().is_err());
    assert_eq!(f.pause, before_pause);
    assert_eq!(format!("{:?}", f.presentation), before_presentation);
    assert_eq!(f.device.owner.mixer().frame_cursor(), physical);
    assert_eq!(&*f.runtime.transport_mut(), &transport);
    assert_eq!(f.device.step, 0);
    assert!(f.host.reports.is_empty());
}

#[test]
fn target_missing_first_compound_publication_cannot_enqueue_pause() {
    let mut f = TargetFixture::new(Rate::NORMAL, None);
    f.device.missing_steps = vec![1, 2];
    f.device.stop_at = 2;
    // The first unseen publication is already more than one second beyond the
    // original cold frame2 observation: refusal is the correct native policy.
    assert!(f
        .run()
        .unwrap_err()
        .to_string()
        .contains("declared age bound"));
    assert_eq!(f.pause.phase(), PausePhase::Running);
    assert!(!f.device.owner.mixer().pause_requested());
    assert!(!f.host.pauses.contains(&PauseState::Pausing));
    assert!(f.host.reports.iter().all(|r| r.input.is_none()));
}

#[test]
fn target_missing_resume_packet_preserves_paused_transport_without_early_ack() {
    let mut f = TargetFixture::new(Rate::NORMAL, None);
    f.device.missing_steps = vec![5];
    f.device.stop_at = 5;
    assert!(f.run().unwrap().is_none());
    assert_eq!(f.pause.phase(), PausePhase::Resuming);
    assert!(f.runtime.transport_mut().is_paused());
    assert!(f.host.pauses.contains(&PauseState::Paused));
    assert!(f.host.pauses.contains(&PauseState::Resuming));
    assert!(!f.host.report_steps.contains(&5));
    assert_eq!(f.device.hold_calls, vec![(2, true), (4, false)]);
    assert!(!f.device.held);
}

#[test]
fn target_actual_held_callbacks_freeze_original_mixer_and_converter_until_resume_request() {
    let mut f = TargetFixture::new(Rate::NORMAL, None);
    assert!(f.run().unwrap().is_none());
    assert_eq!(f.device.hold_calls, vec![(2, true), (4, false)]);
    assert!(!f.device.held);
    assert_eq!(f.device.held_callbacks.len(), 2);
    for (step, before_frame, after_frame, before_position, after_position) in
        &f.device.held_callbacks
    {
        assert!(*step == 3 || *step == 4);
        assert_eq!(
            before_frame, after_frame,
            "held output must not advance physical Mixer history"
        );
        assert_eq!(before_position.frame, after_position.frame);
        assert_eq!(before_position.numerator, after_position.numerator);
        assert_eq!(before_position.denominator, after_position.denominator);
    }
    let held_source_end = f.device.held_callbacks.last().unwrap().2;
    assert!(f
        .device
        .acquired
        .iter()
        .any(|(step, report, boundaries)| *step >= 5
            && report
                .is_some_and(|r| !r.paused && r.start_frame + r.frames as u64 > held_source_end)
            && boundaries.resume.is_some()));
    assert_eq!(f.pause.phase(), PausePhase::Running);
    assert!(!f.runtime.transport_mut().is_paused());
}

#[test]
fn target_real_pause_resume_reconciles_original_release_at_frozen_song_without_new_input_watermark()
{
    let mut f = TargetFixture::new(Rate::NORMAL, None);
    f.device
        .original_events
        .push_back(input(1_500_000_100, 0, ButtonState::Down));
    f.device
        .original_events
        .push_back(input(4_000_000_100, 1, ButtonState::Up));
    assert!(f.run().unwrap().is_none());
    let release = f
        .host
        .reports
        .iter()
        .find(|report| {
            report
                .input
                .as_ref()
                .is_some_and(|event| event.meta().sequence == 1)
        })
        .unwrap();
    let meta = release.input.as_ref().unwrap().meta();
    assert_eq!(meta.source, DeviceId(9));
    assert_eq!(meta.original_clock_point, Some(cp(1, 4_000_000_100)));
    assert_eq!(meta.clock_domain, ClockDomainId(3));
    let (_, report, _) = f
        .device
        .acquired
        .iter()
        .find(|(_, _, boundaries)| boundaries.pause.is_some())
        .unwrap();
    let frozen = Timestamp::from_nanos(
        ((report.unwrap().playback_start_frame - 1) as i128 * 1_000_000_000 / 3) as i64,
    );
    assert_eq!(release.song_time, frozen);
    assert_eq!(
        f.presentation.authority().committed_input_host(),
        Some(cp(1, 1_500_000_100))
    );
    assert!(f
        .capture
        .as_ref()
        .unwrap()
        .records()
        .iter()
        .any(|record| record.song_time == frozen
            && matches!(record.operation, ReplayOperation::Input(_))));
}

fn earlier(a: InterpolatedTargetTime, b: InterpolatedTargetTime) -> InterpolatedTargetTime {
    if a.compare_interpolated_time(b).is_gt() {
        b
    } else {
        a
    }
}

#[test]
fn target_retirement_dispatch_runs_once_before_input_acquisition_at_two_real_rates() {
    for rate in [Rate::NORMAL, Rate::new(3, 2).unwrap()] {
        let mut f = TargetFixture::new(rate, None);
        f.host.pause_enabled = false;
        f.device.stop_at = 4;
        f.device
            .original_events
            .push_back(input(1_500_000_100, 0, ButtonState::Down));
        assert!(f.run().unwrap().is_none());
        assert_eq!(f.device.retirement_calls.len(), 4);
        for (i, call) in f.device.retirement_calls.iter().enumerate() {
            assert_eq!(call.step, i + 1);
            assert_eq!(
                call.acquire_calls, i,
                "dispatch precedes this lap's acquisition"
            );
            assert_eq!(call.identity, f.cfg.source_history);
            assert_eq!(call.source_minimum, None);
            assert!(call
                .query
                .compare_interpolated_time(call.future_input)
                .is_eq());
        }
        assert_eq!(f.device.retirement_calls[0].committed_input, None);
        assert_eq!(
            f.presentation.authority().committed_input_host(),
            Some(cp(1, 1_500_000_100))
        );
        assert!(f
            .capture
            .as_ref()
            .unwrap()
            .records()
            .iter()
            .any(|r| matches!(r.operation, ReplayOperation::Input(_))));
    }
}

#[test]
fn target_retirement_port_error_preserves_pending_original_pause_and_committed_prefix() {
    // The ordinary host requests pause on lap one, which deliberately fences
    // its acquired input until the next native ACK. Let one real lap commit
    // before requesting pause, then fail retirement before that pause's ACK.
    struct AfterFirstLap<'a>(&'a mut Host);
    impl NativeGameplayHost for AfterFirstLap<'_> {
        fn cancelled(&self) -> bool {
            self.0.cancelled()
        }
        fn pause_requested(&self) -> bool {
            self.0.shared.get() >= 2 && self.0.pause_requested()
        }
        fn retry_pause_publication(&mut self) {
            self.0.retry_pause_publication();
        }
        fn publish_pause(&mut self, state: PauseState) {
            self.0.publish_pause(state);
        }
        fn publish_section_end(&mut self, at: Timestamp) {
            self.0.publish_section_end(at);
        }
        fn publish_report(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
            self.0.publish_report(report)
        }
        fn publish_local_reports(
            &mut self,
            reports: &[crate::local_runtime::PlayerReport],
        ) -> NativeGameplayResult<()> {
            self.0.publish_local_reports(reports)
        }
        fn diagnostic(&mut self, diagnostic: NativeGameplayDiagnostic<'_>) {
            self.0.diagnostic(diagnostic);
        }
    }
    let mut f = TargetFixture::new(Rate::NORMAL, None);
    f.device
        .original_events
        .push_back(input(1_500_000_100, 0, ButtonState::Down));
    f.device
        .original_events
        .push_back(input(3_500_000_100, 1, ButtonState::Up));
    f.device.retirement_error_step = Some(3);
    let result = run_gameplay_audio_coordinate_with_result_and_ports(
        &mut f.device,
        AudioGameplaySession {
            session: GameplaySession {
                runtime: &mut f.runtime,
                gauge: &mut f.gauge,
                bgm: &mut f.bgm,
                discipline: &mut f.presentation,
                pause: &mut f.pause,
                end: &mut f.end,
                completion: &mut f.completion,
                capture: &mut f.capture,
                competition: &mut f.competition,
                delivery: &mut f.delivery,
                pre_origin_inputs: &mut f.pre,
            },
            merger: &mut f.merger,
        },
        f.cfg,
        &mut Control::default(),
        &mut AfterFirstLap(&mut f.host),
        Some(f.speed),
    );
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("scripted retirement dispatch refusal"));
    assert_eq!(f.device.acquire_calls, 2);
    assert_eq!(f.device.acquired_events, 1);
    assert_eq!(f.device.original_events.len(), 1);
    assert_eq!(f.pause.phase(), PausePhase::Pausing);
    assert!(!f.runtime.transport_mut().is_paused());
    assert!(f.device.hold_calls.is_empty());
    assert!(f.host.report_steps.iter().all(|step| *step == 1));
    assert_eq!(
        f.presentation.authority().committed_input_host(),
        Some(cp(1, 1_500_000_100))
    );
    let call = f.device.retirement_calls.last().unwrap();
    let pending = f
        .pause
        .pending_coordinate_retention_query()
        .unwrap()
        .unwrap();
    let staged = f
        .pause
        .prepare_coordinate_adoption(
            f.device.owner.last_real_source_report(),
            Some(f.device.owner.boundaries()),
        )
        .unwrap()
        .unwrap();
    assert!(call
        .query
        .compare_interpolated_time(earlier(
            earlier(call.future_input, pending),
            staged.exact_cut()
        ))
        .is_eq());
    let latest = exact(
        f.device
            .basis
            .start_time()
            .checked_add_frames(f.device.frame(), 7)
            .unwrap(),
    );
    assert!(
        call.query.compare_interpolated_time(latest).is_lt(),
        "latest output alone loses the pending cut"
    );
    assert!(f
        .capture
        .as_ref()
        .unwrap()
        .records()
        .iter()
        .filter_map(|r| match &r.operation {
            ReplayOperation::Input(input) => Some(input.physical.meta().sequence),
            _ => None,
        })
        .all(|sequence| sequence == 0));
}

#[test]
fn target_busy_retirement_cannot_acknowledge_missing_pause_report() {
    let mut f = TargetFixture::new(Rate::NORMAL, None);
    f.device.retirement_busy = true;
    f.device.report_missing_steps = vec![2, 3, 4];
    f.device.stop_at = 4;
    f.device
        .original_events
        .push_back(input(3_000_000_100, 0, ButtonState::Down));
    assert!(f.run().unwrap().is_none());
    assert_eq!(f.device.retirement_calls.len(), 4);
    assert_eq!(f.pause.phase(), PausePhase::Pausing);
    assert!(!f.runtime.transport_mut().is_paused());
    assert!(!f.host.pauses.contains(&PauseState::Paused));
    assert!(f.device.hold_calls.is_empty());
    assert_eq!(f.presentation.authority().committed_input_host(), None);
    let pending = f
        .pause
        .pending_coordinate_retention_query()
        .unwrap()
        .unwrap();
    for call in f.device.retirement_calls.iter().skip(1) {
        assert!(call
            .query
            .compare_interpolated_time(earlier(call.future_input, pending))
            .is_eq());
    }
    assert!(f.host.reports.iter().all(|r| r.input.is_none()));
}

#[test]
fn target_busy_retirement_leaves_real_pause_resume_and_canonical_capture_unchanged() {
    let mut ordinary = TargetFixture::new(Rate::new(3, 2).unwrap(), None);
    let mut busy = TargetFixture::new(Rate::new(3, 2).unwrap(), None);
    ordinary
        .device
        .original_events
        .push_back(input(1_500_000_100, 0, ButtonState::Down));
    busy.device
        .original_events
        .push_back(input(1_500_000_100, 0, ButtonState::Down));
    busy.device.retirement_busy = true;
    assert!(ordinary.run().unwrap().is_none());
    assert!(busy.run().unwrap().is_none());
    assert_eq!(busy.pause.phase(), PausePhase::Running);
    assert_eq!(busy.device.hold_calls, ordinary.device.hold_calls);
    assert_eq!(busy.host.pauses, ordinary.host.pauses);
    assert_eq!(busy.host.report_steps, ordinary.host.report_steps);
    assert_eq!(
        busy.capture.as_ref().unwrap().records(),
        ordinary.capture.as_ref().unwrap().records()
    );
    assert_eq!(
        busy.presentation.authority().committed_input_host(),
        ordinary.presentation.authority().committed_input_host()
    );
    assert_eq!(
        busy.device.owner.mixer().frame_cursor(),
        ordinary.device.owner.mixer().frame_cursor()
    );
    assert_eq!(busy.device.report, ordinary.device.report);
    assert_eq!(
        busy.device.retirement_calls.len(),
        ordinary.device.retirement_calls.len()
    );
}

#[test]
fn target_stale_original_control_is_rejected_before_retirement_dispatch_and_new_input() {
    let mut f = TargetFixture::new(Rate::NORMAL, None);
    f.device.stale_step = Some(2);
    f.device
        .original_events
        .push_back(input(2_500_000_100, 0, ButtonState::Down));
    assert!(f.run().is_err());
    assert_eq!(f.device.retirement_calls.len(), 1);
    assert_eq!(f.device.retirement_calls[0].step, 1);
    assert_eq!(f.device.acquire_calls, 1);
    assert_eq!(f.device.acquired_events, 0);
    assert_eq!(f.device.original_events.len(), 1);
    assert_eq!(f.presentation.authority().committed_input_host(), None);
    assert!(f.device.hold_calls.is_empty());
    assert!(f.host.report_steps.iter().all(|step| *step == 1));
}

#[test]
fn target_first_retirement_error_precedes_input_runtime_and_capture_effects() {
    let mut f = TargetFixture::new(Rate::NORMAL, None);
    f.host.pause_enabled = false;
    f.device.retirement_error_step = Some(1);
    f.device
        .original_events
        .push_back(input(1_500_000_100, 0, ButtonState::Down));
    let before_transport = f.runtime.transport_mut().clone();
    assert!(f
        .run()
        .unwrap_err()
        .to_string()
        .contains("scripted retirement dispatch refusal"));
    assert_eq!(f.device.retirement_calls.len(), 1);
    assert_eq!(f.device.acquire_calls, 0);
    assert_eq!(f.device.acquired_events, 0);
    assert_eq!(f.device.original_events.len(), 1);
    assert_eq!(&*f.runtime.transport_mut(), &before_transport);
    assert!(f.host.reports.is_empty());
    assert!(f.capture.as_ref().unwrap().records().is_empty());
    assert_eq!(f.presentation.authority().committed_input_host(), None);
    assert!(f.presentation.authority().committed_operation().is_none());
}

#[test]
fn target_retirement_dispatch_includes_actual_finite_endpoint_before_acquisition() {
    let mut f = TargetFixture::new(Rate::NORMAL, Some(Timestamp::from_nanos(2_000_000_000)));
    f.host.pause_enabled = false;
    f.device.retirement_error_step = Some(1);
    assert!(f
        .run()
        .unwrap_err()
        .to_string()
        .contains("scripted retirement dispatch refusal"));
    let call = f.device.retirement_calls.last().unwrap();
    let end = exact(f.device.owner.boundaries().end.unwrap().target_time);
    assert!(call
        .query
        .compare_interpolated_time(earlier(call.future_input, end))
        .is_eq());
    assert_eq!(f.device.acquire_calls, 0);
    assert!(f.host.reports.is_empty());
    assert!(f.capture.as_ref().unwrap().records().is_empty());
    assert!(f.presentation.authority().committed_operation().is_none());
}

#[test]
fn target_retirement_minimum_stays_conservative_with_actual_input_backlog_and_pending_pause() {
    let mut f = TargetFixture::new(Rate::NORMAL, None);
    f.device.retirement_busy = true;
    f.device.report_missing_steps = vec![2, 3, 4];
    f.device.backlog_steps = vec![1, 2, 3, 4];
    f.device.stop_at = 4;
    f.device
        .original_events
        .push_back(input(1_500_000_100, 0, ButtonState::Down));
    assert!(f.run().unwrap().is_none());
    assert_eq!(f.pause.phase(), PausePhase::Pausing);
    assert_eq!(f.presentation.authority().committed_input_host(), None);
    assert!(f.host.reports.iter().all(|r| r.input.is_none()));
    let pending = f
        .pause
        .pending_coordinate_retention_query()
        .unwrap()
        .unwrap();
    assert_eq!(f.device.retirement_calls.len(), 4);
    for call in f.device.retirement_calls.iter().skip(1) {
        assert!(call
            .query
            .compare_interpolated_time(earlier(call.future_input, pending))
            .is_eq());
    }
}
