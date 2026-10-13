//! The production cohort loop, using actual converted generation and Mixer adoption.
//! Supplied native observations are DI evidence, not physical audio measurements.
use super::*;
use crate::{
    gameplay::output::ports::{
        NativeTargetPlaybackFacts, NativeTargetPlaybackObservation, NativeTargetPlaybackQuery,
    },
    native_audio_presentation::TargetNativeAudioSnapshot,
    native_coordinate_control::{CoordinatePlaybackControlFacts, TargetCoordinateControlFacts},
    native_output_coordinate::CoordinateTargetNativeAudioSnapshot,
    native_playback_phase::ConvertedPlaybackFacts,
};
use beatkernel_platform::audio::{ConvertedNativeOutputState, DeviceFormat, SampleEncoding};

fn exact_target(time: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap()
}
fn target_observation(
    basis: TargetFrameBasis,
    frame: u64,
    host: i64,
) -> CoordinateTargetNativeAudioSnapshot {
    CoordinateTargetNativeAudioSnapshot::new(
        TargetNativeAudioSnapshot {
            epoch: 7,
            basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(frame).unwrap(),
                target: cp(1, host),
            }),
        },
        NativeOutputCoordinate::StreamFrame { frame },
    )
    .unwrap()
}

struct RetirementCall {
    step: usize,
    minimum: InterpolatedTargetTime,
    future: InterpolatedTargetTime,
    identity: SourcePlaybackIdentity,
    source_minimum: Option<u64>,
    acquisitions: usize,
    committed_input: Option<ClockPoint>,
    end: Option<TargetTime>,
    closed_prefix: Option<ClockPoint>,
    native_query: InterpolatedTargetTime,
    audio_held: bool,
}

struct TargetDevice {
    output: ConvertedNativeOutputState,
    converted: ConvertedRenderReport,
    basis: TargetFrameBasis,
    step: Rc<Cell<usize>>,
    rendered_step: usize,
    missing_at: Option<usize>,
    stale_at: Option<usize>,
    omit_at: Option<usize>,
    previous: Option<CoordinatePlaybackControlFacts>,
    events: VecDeque<PhysicalInputEvent>,
    calls: usize,
    forbidden_calls: usize,
    close_at: usize,
    prefix_held_until: usize,
    inputs_at_observation: Vec<(usize, Option<ClockPoint>)>,
    held: bool,
    held_requests: Vec<(usize, bool)>,
    held_callbacks: usize,
    retirement_calls: Vec<RetirementCall>,
    retirement_busy: bool,
    retirement_error_at: Option<usize>,
    acquisitions: usize,
}
impl TargetDevice {
    fn host(&self) -> i64 {
        104 + 4 * self.step.get() as i64
    }
    fn frame(&self) -> u64 {
        1 + self.step.get() as u64
    }
    fn packet(
        &self,
        q: InterpolatedTargetTime,
        id: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<NativeTargetPlaybackQuery<OriginalNativePresentationEvidence>> {
        let history = self.output.phase_history().unwrap();
        let mut selected = None;
        for index in 0..history.len() {
            let span = history.span(index).unwrap();
            let words = span.to_words();
            let start = TargetTime::new(words[4], words[5], words[6])?;
            if !q.compare_target_time(start).is_lt() {
                selected = Some(index);
            }
        }
        let index = selected.expect("actual generated segment covers admitted query");
        let phase = history.span(index).unwrap();
        let segment_end = history
            .span(index + 1)
            .map(|span| {
                let words = span.to_words();
                TargetTime::new(words[4], words[5], words[6]).unwrap()
            })
            .unwrap_or(history.end_time());
        let source_history = self
            .output
            .converter_owner()
            .source_playback_history()
            .unwrap();
        let playback = (0..source_history.len())
            .filter_map(|index| source_history.span(index))
            .find(|span| {
                !phase
                    .compare_source_frame_at_interpolated_time(
                        q,
                        segment_end,
                        span.physical_start(),
                    )
                    .unwrap()
                    .is_lt()
                    && span.physical_end().is_none_or(|end| {
                        phase
                            .compare_source_frame_at_interpolated_time(q, segment_end, end)
                            .unwrap()
                            .is_lt()
                    })
            })
            .expect("original Mixer source span covers exact converter coordinate");
        let coordinate = target_observation(self.basis, self.frame(), self.host());
        Ok(NativeTargetPlaybackQuery::new(
            7,
            self.basis,
            id,
            q,
            NativeTargetPlaybackFacts {
                playback: ConvertedPlaybackFacts {
                    phase,
                    segment_end,
                    generated_target_end: history.end_time(),
                    submitted_target_end: self.output.admitted_target_time().unwrap(),
                    playback,
                    pulled_source_end: self.output.mixer().frame_cursor(),
                    revision: 2 + 2 * self.step.get() as u64,
                },
                source: if self.omit_at == Some(self.step.get()) {
                    None
                } else {
                    self.output.last_real_source_report()
                },
                converted: Some(self.converted),
                boundaries: self.output.boundaries(),
                revision: 20 + 2 * self.step.get() as u64,
            },
            Some(coordinate.snapshot().evidence),
        )?)
    }
}
impl GameplayDevice for TargetDevice {
    type Presentation = PresentationEstimator;
    fn request_history_retirement(
        &mut self,
        p: &NativeAudioPresentation,
        minimum: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
        source_minimum: Option<u64>,
    ) -> NativeGameplayResult<Option<u64>> {
        // Record pure math only. The genuine owner above remains the sole
        // source of native observations and generated/admitted playback facts.
        assert_eq!(p.authority().epoch().id, 7);
        assert_eq!(
            p.authority().coordinate_basis(),
            Some(AudioAuthorityCoordinateBasis::TargetFrame(self.basis))
        );
        let NativeOutputCoordinate::StreamFrame { frame } = p
            .authority()
            .latest_coordinate_observation()
            .unwrap()
            .coordinate()
            .unwrap()
        else {
            panic!("actual Target frame fixture has original frame evidence");
        };
        self.retirement_calls.push(RetirementCall {
            step: self.step.get(),
            minimum: *minimum,
            future: p.authority().future_input_retention_query()?,
            identity,
            source_minimum,
            acquisitions: self.acquisitions,
            committed_input: p.authority().committed_input_host(),
            end: self.output.boundaries().end.map(|end| end.target_time),
            closed_prefix: p.authority().closed_host_prefix(),
            native_query: exact_target(self.basis.start_time().checked_add_frames(frame, 7)?),
            audio_held: self.held,
        });
        if self.retirement_error_at == Some(self.step.get()) {
            return Err("cohort retirement delegate failure".into());
        }
        Ok((!self.retirement_busy).then_some(2))
    }
    fn observe_end(
        &mut self,
        _: &mut NativeEnd,
        _: &PresentationEstimator,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<crate::native_end::EndBoundary>> {
        self.forbidden_calls += 1;
        Err("scalar endpoint forbidden in Target coordinate entry".into())
    }
    fn playback_mapping_at(
        &self,
        _: &NativeAudioPresentation,
        q: &InterpolatedTargetTime,
        id: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<NativePlaybackMapping>> {
        Ok(Some(NativePlaybackMapping::from_target(
            &self.packet(*q, id)?,
        )))
    }
    fn observe_coordinate_playback_control(
        &mut self,
        p: &mut NativeAudioPresentation,
        id: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinatePlaybackControlFacts>> {
        self.calls += 1;
        self.inputs_at_observation
            .push((self.step.get(), p.authority().committed_input_host()));
        if self.missing_at == Some(self.step.get()) {
            return Ok(None);
        }
        if self.rendered_step < self.step.get() {
            self.converted = if self.held {
                let physical = self.output.mixer().frame_cursor();
                let position = self.converted.source_position;
                let source = self.output.last_real_source_report();
                let boundaries = self.output.boundaries();
                let held = self.output.render_held_pending(1)?;
                assert_eq!(held.state, ConvertedOutputState::Held);
                assert_eq!(held.source, None);
                assert_eq!(held.source_start_position, position);
                assert_eq!(held.source_position, position);
                assert_eq!(self.output.mixer().frame_cursor(), physical);
                assert_eq!(self.output.last_real_source_report(), source);
                assert_eq!(self.output.boundaries(), boundaries);
                self.held_callbacks += 1;
                held
            } else {
                self.output.render_pending(1)?
            };
            self.output.admit(1)?;
            self.rendered_step = self.step.get();
        }
        let coordinate = target_observation(self.basis, self.frame(), self.host());
        p.admit_target_coordinate(coordinate)?;
        if self.stale_at == Some(self.step.get()) {
            return Ok(self.previous);
        }
        let q = exact_target(
            self.basis
                .start_time()
                .checked_add_frames(self.frame(), 7)?,
        );
        let native = NativeTargetPlaybackObservation::new(coordinate, self.packet(q, id)?)?;
        let facts = CoordinatePlaybackControlFacts::Target(
            TargetCoordinateControlFacts::from_target(&native, p, id)?,
        );
        self.previous = Some(facts);
        Ok(Some(facts))
    }
    fn observe_coordinate_control(
        &mut self,
        _: &mut NativeAudioPresentation,
        _: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinateControlFacts>> {
        self.forbidden_calls += 1;
        Err("Target must use the common compound port".into())
    }
    fn observe(&mut self, _: &mut PresentationEstimator) -> NativeGameplayResult<()> {
        self.forbidden_calls += 1;
        Err("scalar observation forbidden".into())
    }
    fn observe_audio(&mut self, _: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
        self.forbidden_calls += 1;
        Err("independent native observation forbidden".into())
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        Ok(self.output.last_real_source_report())
    }
    fn set_audio_held(&mut self, held: bool) -> NativeGameplayResult<()> {
        self.held_requests.push((self.step.get(), held));
        self.held = held;
        Ok(())
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(cp(1, self.host()))
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        self.acquisitions += 1;
        while self
            .events
            .front()
            .is_some_and(|event| event.meta().timestamp <= cp(1, self.host()).timestamp)
        {
            events.push_back(self.events.pop_front().unwrap());
        }
        Ok(InputBatch {
            completed_through: Some(cp(
                1,
                if self.step.get() < self.prefix_held_until {
                    102
                } else {
                    self.host()
                },
            )),
            backlog: false,
            closed: self.step.get() >= self.close_at,
        })
    }
    fn seed_resume(
        &mut self,
        _: &mut PresentationEstimator,
        _: ClockPair,
    ) -> NativeGameplayResult<()> {
        self.forbidden_calls += 1;
        Err("Target resume cannot reseed scalar time".into())
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        Ok(cp(2, 777))
    }
    fn publish_paused_audio_output(
        &mut self,
        _: crate::gameplay_presentation::GameplayAudioOutputContext<'_>,
        _: ClockPoint,
    ) -> NativeGameplayResult<bool> {
        Ok(false)
    }
}

struct TargetFixture {
    group: RuntimeGroup,
    states: Vec<GameplayPlayerState<NoopSoloCompetition>>,
    merger: InputMerger,
    p: NativeAudioPresentation,
    bgm: BgmFeeder,
    pause: NativePause,
    end: Option<NativeEnd>,
    delivery: InputDeliveryTelemetry,
    pre: u64,
    device: TargetDevice,
    control: Control,
    host: Host,
    config: CoordinateGameplayConfig,
    speed: LiveSpeedSetup,
    phase: OutputPhaseBasis,
}
impl TargetFixture {
    fn new(ids: &[u32], rate: Rate, finite: bool) -> Self {
        // Reuse the existing chart, captures and real RuntimeGroup command queue.
        // Move its Mixer into one converter; no second engine or producer exists.
        let Fixture {
            group,
            states,
            merger,
            bgm,
            pause,
            end,
            delivery,
            pre,
            device,
            control,
            mut host,
            mut config,
            speed,
            phase,
            ..
        } = Fixture::new(ids, rate, finite);
        let mut output = ConvertedNativeOutputState::new(
            device.mixer,
            DeviceFormat::new(7, 1, SampleEncoding::Float32, None).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            ResampleQuality::Linear,
            8,
        )
        .unwrap_or_else(|_| panic!("valid converter"));
        output.enable_phase_history(8).unwrap();
        let basis = output.target_frame_basis();
        let converted = output.render_pending(1).unwrap();
        output.admit(1).unwrap();
        let mut p = new_coordinate_audio_presentation(
            7,
            AudioAuthorityCoordinateBasis::TargetFrame(basis),
            ClockDomainId(1),
            cp(3, 500),
            AudioAuthorityConfig {
                history_capacity: 16,
                max_observation_age: Duration::from_nanos(100),
                input_extrapolation: ExtrapolationPolicy::Bounded {
                    before: Duration::ZERO,
                    after: Duration::ZERO,
                },
                max_input_ahead: Duration::ZERO,
            },
        )
        .unwrap();
        p.admit_target_coordinate(target_observation(basis, 0, 100))
            .unwrap();
        p.admit_target_coordinate(target_observation(basis, 1, 104))
            .unwrap();
        config.audio.gameplay.stream_origin = p.authority().epoch().stream_origin;
        config.audio.gameplay.playback_origin = p.authority().epoch().stream_origin;
        host.pause_until = if finite { 0 } else { 8 };
        Self {
            group,
            states,
            merger,
            p,
            bgm,
            pause: pause.with_target_basis(7, basis).unwrap(),
            end: end.map(|end| end.with_target_basis(7, basis).unwrap()),
            delivery,
            pre,
            control,
            host,
            config,
            speed,
            phase,
            device: TargetDevice {
                output,
                converted,
                basis,
                step: device.step,
                rendered_step: 0,
                missing_at: None,
                stale_at: None,
                omit_at: None,
                previous: None,
                events: VecDeque::new(),
                calls: 0,
                forbidden_calls: 0,
                close_at: 12,
                prefix_held_until: 0,
                inputs_at_observation: vec![],
                held: false,
                held_requests: vec![],
                held_callbacks: 0,
                retirement_calls: vec![],
                retirement_busy: false,
                retirement_error_at: None,
                acquisitions: 0,
            },
        }
    }
    fn run(&mut self) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
        let session: AudioCohortSession<'_, NoopSoloCompetition, NoopGroupCompetition> =
            CohortSession {
                group: &mut self.group,
                network: None,
                states: &mut self.states,
                merger: &mut self.merger,
                bgm: &mut self.bgm,
                discipline: &mut self.p,
                pause: &mut self.pause,
                end: &mut self.end,
                delivery: &mut self.delivery,
                pre_origin_inputs: &mut self.pre,
            };
        run_cohort_audio_coordinate_with_results_and_ports(
            &mut self.device,
            session,
            self.config,
            &mut self.control,
            &mut self.host,
            Some(self.speed),
        )
    }
    fn assert_replays(&self) {
        for state in &self.states {
            let capture = state.capture.as_ref().unwrap();
            let replay = ReplaySession::from_records(
                capture.header().clone(),
                member(state.player.0).judge,
                capture.records().to_vec(),
            )
            .unwrap();
            assert_eq!(
                replay.engine().stable_hash().unwrap(),
                self.group
                    .member_judge(state.player)
                    .unwrap()
                    .stable_hash()
                    .unwrap()
            );
        }
    }
}

#[test]
fn actual_converted_two_and_four_members_pause_resume_one_shared_phase_and_replay() {
    for ids in [&[7, 29][..], &[7, 29, 101, 409][..]] {
        for rate in [
            Rate::NORMAL,
            Rate::new(1, 2).unwrap(),
            Rate::new(3, 2).unwrap(),
            Rate::new(2, 1).unwrap(),
        ] {
            let mut f = TargetFixture::new(ids, rate, false);
            assert!(f.run().unwrap().is_none());
            assert_eq!(f.device.forbidden_calls, 0);
            assert!(f.device.calls >= 8);
            assert!(f.host.pauses.contains(&PauseState::Paused));
            assert!(f.host.pauses.contains(&PauseState::Resuming));
            assert_eq!(f.pause.phase(), PausePhase::Running);
            assert_eq!(f.group.transport().output_phase_basis(), Some(f.phase));
            let songs: Vec<Vec<_>> = ids
                .iter()
                .map(|id| {
                    f.host
                        .reports
                        .iter()
                        .filter(|report| report.player == PlayerId(*id))
                        .map(|report| report.report.song_time)
                        .collect()
                })
                .collect();
            assert!(!songs[0].is_empty());
            assert!(songs.iter().all(|song| song == &songs[0]));
            assert!(f.device.output.boundaries().resume.is_some());
            assert!(f.device.held_callbacks > 0);
            assert!(f.device.held_requests.iter().any(|(_, held)| *held));
            assert!(f
                .device
                .held_requests
                .iter()
                .any(|(step, held)| *step >= 8 && !held));
            assert!(!f.device.held);
            assert_eq!(f.host.completed, 0);
            assert!(!f.group.poisoned());
            f.assert_replays();
        }
    }
}

#[test]
fn converted_pause_equality_and_paused_release_retain_original_member_device() {
    let mut f = TargetFixture::new(&[7, 29, 101, 409], Rate::NORMAL, false);
    for id in [409, 101, 29, 7] {
        f.device
            .events
            .push_back(button(id, 104, 1, ButtonState::Down));
    }
    for id in [409, 101, 29, 7] {
        f.device
            .events
            .push_back(button(id, 124, 2, ButtonState::Up));
    }
    assert!(f.run().unwrap().is_none());
    assert_eq!(f.merger.pending(), 0);
    for report in f
        .host
        .reports
        .iter()
        .filter(|report| report.report.input.is_some())
    {
        let event = report.report.input.as_ref().unwrap();
        assert_eq!(event.meta().source, DeviceId(u64::from(report.player.0)));
        assert!(event.meta().original_clock_point.is_some());
    }
    assert_eq!(f.device.forbidden_calls, 0);
    f.assert_replays();
}

#[test]
fn missing_target_packet_does_not_ack_or_commit_or_consume_original_input() {
    let mut f = TargetFixture::new(&[7, 29], Rate::NORMAL, false);
    f.device.missing_at = Some(0);
    f.device.close_at = 0;
    f.device
        .events
        .push_back(button(7, 103, 1, ButtonState::Down));
    assert!(f.run().unwrap().is_none());
    assert_eq!(f.pause.phase(), PausePhase::Running);
    assert!(f.host.reports.is_empty());
    assert!(f.p.authority().committed_coordinate_operation().is_none());
    assert!(f.p.authority().committed_input_host().is_none());
    assert_eq!(f.device.forbidden_calls, 0);
    assert!(!f.device.held_requests.iter().any(|(_, held)| *held));
}

#[test]
fn stale_target_packet_cannot_ack_pause_or_commit_shared_phase() {
    let mut f = TargetFixture::new(&[7, 29], Rate::NORMAL, false);
    f.device.stale_at = Some(1);
    assert!(f.run().is_err());
    assert!(!f.host.pauses.contains(&PauseState::Paused));
    assert!(f.p.authority().committed_coordinate_operation().is_none());
    assert!(f.host.reports.is_empty());
    assert!(!f.group.poisoned());
    assert_eq!(f.device.forbidden_calls, 0);
    assert!(!f.device.held_requests.iter().any(|(_, held)| *held));
}

#[test]
fn coherent_target_packet_without_source_report_holds_input_until_genuine_adoption() {
    let mut f = TargetFixture::new(&[7, 29], Rate::NORMAL, false);
    f.device.omit_at = Some(1);
    f.device.prefix_held_until = 4;
    f.device
        .events
        .push_back(button(7, 103, 1, ButtonState::Down));
    f.device
        .events
        .push_back(button(7, 112, 2, ButtonState::Up));
    assert!(f.run().unwrap().is_none());
    assert_eq!(
        f.device
            .inputs_at_observation
            .iter()
            .find(|(step, _)| *step == 2)
            .unwrap()
            .1,
        None
    );
    assert!(f.host.pauses.contains(&PauseState::Paused));
    assert_eq!(f.pause.phase(), PausePhase::Running);
    assert!(f
        .host
        .reports
        .iter()
        .filter_map(|report| report.report.input.as_ref())
        .any(|event| event.meta().original_clock_point == Some(cp(1, 103))));
    f.assert_replays();
}

#[test]
fn converted_real_endpoint_completes_every_member_and_canonical_capture() {
    for ids in [&[7, 29][..], &[7, 29, 101, 409][..]] {
        let mut f = TargetFixture::new(ids, Rate::NORMAL, true);
        let results = f
            .run()
            .unwrap()
            .expect("actual source end projected into generated and submitted target");
        assert_eq!(results.len(), ids.len());
        assert_eq!(f.host.completed, 1);
        assert_eq!(f.host.ends, [Timestamp::from_nanos(500_000_000)]);
        assert!(f
            .states
            .iter()
            .all(|state| state.last_song == Timestamp::from_nanos(500_000_000)));
        assert_eq!(f.merger.pending(), 0);
        assert_eq!(f.device.forbidden_calls, 0);
        assert!(f.device.output.boundaries().end.is_some());
        f.assert_replays();
    }
}

#[test]
fn target_publication_failure_keeps_genuine_shared_ack_and_all_member_report_prefixes() {
    let mut f = TargetFixture::new(&[7, 29, 101, 409], Rate::NORMAL, false);
    f.host.fail_reports = true;
    let error = f.run().unwrap_err();
    assert!(error
        .downcast_ref::<NativeCohortObservationError>()
        .is_some());
    assert_eq!(f.pause.phase(), PausePhase::Paused);
    assert!(f.group.transport().is_paused());
    assert!(f.p.authority().committed_coordinate_operation().is_some());
    assert!(f.device.held);
    assert!(f.device.held_requests.iter().any(|(_, held)| *held));
    assert_eq!(f.host.reports.len(), 4);
    assert!(f
        .states
        .iter()
        .all(|state| state.capture.as_ref().unwrap().records().len() == 1));
    assert!(!f.group.poisoned());
}

#[test]
fn cohort_retirement_is_once_per_shared_iteration_for_two_and_four_players() {
    for ids in [&[7, 29][..], &[7, 29, 101, 409][..]] {
        let mut f = TargetFixture::new(ids, Rate::NORMAL, false);
        assert!(f.run().unwrap().is_none());
        assert_eq!(f.device.retirement_calls.len(), f.device.calls);
        for (step, call) in f.device.retirement_calls.iter().enumerate() {
            assert_eq!(call.step, step);
            assert_eq!(call.acquisitions, step, "dispatch precedes acquisition");
            assert_eq!(call.identity, f.config.source_history);
            assert_eq!(call.source_minimum, None, "no invented practice Source pin");
            assert!(!call.minimum.compare_interpolated_time(call.future).is_gt());
        }
        assert!(f.device.retirement_calls.len() > 4);
        assert_eq!(f.device.forbidden_calls, 0);
        assert!(!f.group.poisoned());
        f.assert_replays();
    }
}

#[test]
fn cohort_retirement_keeps_pending_pause_while_native_advances_and_shared_prefix_stalls() {
    let mut f = TargetFixture::new(&[7, 29, 101, 409], Rate::NORMAL, false);
    let request = exact_target(
        f.device
            .basis
            .start_time()
            .checked_add_frames(1, 7)
            .unwrap(),
    );
    assert!(f.run().unwrap().is_none());
    let calls = &f.device.retirement_calls;
    let held = calls
        .iter()
        .position(|call| call.audio_held)
        .expect("genuine ACK holds output");
    assert!(
        held > 1,
        "native progress precedes the actual shared pause ACK"
    );
    for call in &calls[1..held] {
        assert!(call.native_query.compare_interpolated_time(request).is_gt());
        assert!(!call.minimum.compare_interpolated_time(request).is_gt());
        assert_eq!(
            call.closed_prefix, calls[0].closed_prefix,
            "unresolved pause prevents closing the shared InputMerger prefix"
        );
        assert!(call
            .future
            .compare_interpolated_time(calls[0].future)
            .is_eq());
    }
    assert!(f.host.pauses.contains(&PauseState::Paused));
    assert_eq!(f.pause.phase(), PausePhase::Running);
    assert_eq!(f.device.forbidden_calls, 0);
    f.assert_replays();
}

#[test]
fn cohort_retirement_protects_real_staged_end_while_shared_input_prefix_is_open() {
    let mut f = TargetFixture::new(&[7, 29, 101, 409], Rate::NORMAL, true);
    f.device.prefix_held_until = 6;
    let results = f
        .run()
        .unwrap()
        .expect("genuine end finishes shared cohort");
    assert_eq!(results.len(), 4);
    let calls: Vec<_> = f
        .device
        .retirement_calls
        .iter()
        .filter(|call| call.end.is_some())
        .collect();
    assert!(
        !calls.is_empty(),
        "retirement dispatch sees the original generated endpoint"
    );
    for call in calls {
        assert!(!call.minimum.compare_target_time(call.end.unwrap()).is_gt());
        assert!(!call.minimum.compare_interpolated_time(call.future).is_gt());
    }
    assert_eq!(f.host.completed, 1);
    assert_eq!(f.device.forbidden_calls, 0);
    assert_eq!(f.merger.pending(), 0);
    f.assert_replays();
}

#[test]
fn cohort_busy_retirement_is_not_pause_ack_and_preserves_all_canonical_captures() {
    let mut f = TargetFixture::new(&[7, 29, 101, 409], Rate::NORMAL, false);
    f.device.retirement_busy = true;
    for id in [409, 101, 29, 7] {
        f.device
            .events
            .push_back(button(id, 104, 1, ButtonState::Down));
    }
    for id in [409, 101, 29, 7] {
        f.device
            .events
            .push_back(button(id, 124, 2, ButtonState::Up));
    }
    assert!(f.run().unwrap().is_none());
    assert_eq!(f.device.retirement_calls.len(), f.device.calls);
    assert!(f.host.pauses.contains(&PauseState::Paused));
    assert!(f.host.pauses.contains(&PauseState::Resuming));
    assert_eq!(f.pause.phase(), PausePhase::Running);
    assert!(
        f.device.held_callbacks > 0,
        "only genuine native adoption holds audio"
    );
    assert_eq!(f.merger.pending(), 0);
    assert!(!f.group.poisoned());
    assert_eq!(f.device.forbidden_calls, 0);
    f.assert_replays();
}

#[test]
fn cohort_initial_retirement_failure_stops_before_input_and_runtime_effects() {
    let mut f = TargetFixture::new(&[7, 29], Rate::NORMAL, false);
    f.device.retirement_error_at = Some(0);
    f.device
        .events
        .push_back(button(7, 103, 1, ButtonState::Down));
    let error = f.run().unwrap_err();
    assert!(error
        .to_string()
        .contains("cohort retirement delegate failure"));
    assert_eq!(f.device.calls, 1);
    assert_eq!(f.device.retirement_calls.len(), 1);
    assert_eq!(f.device.acquisitions, 0);
    assert_eq!(f.device.events.len(), 1);
    assert!(f.host.reports.is_empty());
    assert!(f.p.authority().committed_input_host().is_none());
    assert!(f.p.authority().committed_coordinate_operation().is_none());
    assert!(!f.host.pauses.contains(&PauseState::Paused));
    assert!(f
        .states
        .iter()
        .all(|state| state.capture.as_ref().unwrap().records().is_empty()));
    assert!(!f.group.poisoned());
    assert_eq!(f.device.forbidden_calls, 0);
}

#[test]
fn cohort_late_retirement_failure_preserves_genuine_shared_pause_and_committed_prefix() {
    let mut f = TargetFixture::new(&[7, 29, 101, 409], Rate::NORMAL, false);
    f.device.retirement_error_at = Some(6);
    let error = f.run().unwrap_err();
    assert!(error
        .to_string()
        .contains("cohort retirement delegate failure"));
    assert_eq!(f.device.calls, 7);
    assert_eq!(f.device.acquisitions, 6);
    let call = f.device.retirement_calls.last().unwrap();
    assert_eq!(call.step, 6);
    assert_eq!(call.committed_input, f.p.authority().committed_input_host());
    assert_eq!(f.pause.phase(), PausePhase::Paused);
    assert!(f.group.transport().is_paused());
    assert!(f.device.held);
    assert!(f.p.authority().committed_coordinate_operation().is_some());
    assert!(f
        .states
        .iter()
        .all(|state| !state.capture.as_ref().unwrap().records().is_empty()));
    assert!(!f.group.poisoned());
    assert_eq!(f.device.forbidden_calls, 0);
    f.assert_replays();
}

#[test]
fn stale_or_missing_cohort_packet_cannot_gain_ack_through_retirement_dispatch() {
    let mut stale = TargetFixture::new(&[7, 29], Rate::NORMAL, false);
    stale.device.stale_at = Some(1);
    assert!(stale.run().is_err());
    assert_eq!(
        stale.device.retirement_calls.len(),
        1,
        "invalid packet stops before dispatch"
    );
    assert_eq!(stale.device.acquisitions, 1);
    assert!(!stale.host.pauses.contains(&PauseState::Paused));
    assert!(stale
        .p
        .authority()
        .committed_coordinate_operation()
        .is_none());
    assert!(stale.host.reports.is_empty());
    assert!(!stale.group.poisoned());

    let mut missing = TargetFixture::new(&[7, 29, 101, 409], Rate::NORMAL, false);
    missing.device.missing_at = Some(0);
    missing.device.close_at = 0;
    missing
        .device
        .events
        .push_back(button(7, 103, 1, ButtonState::Down));
    assert!(missing.run().unwrap().is_none());
    assert_eq!(missing.device.calls, 1);
    assert!(missing.device.retirement_calls.len() <= 1);
    assert_eq!(missing.pause.phase(), PausePhase::Running);
    assert!(missing.host.reports.is_empty());
    assert!(missing
        .p
        .authority()
        .committed_coordinate_operation()
        .is_none());
    assert!(missing.p.authority().committed_input_host().is_none());
    assert!(!missing.device.held_requests.iter().any(|(_, held)| *held));
    assert_eq!(missing.device.forbidden_calls, 0);
}
