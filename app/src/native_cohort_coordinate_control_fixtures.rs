//! Actual shared production entry with original software source publications.
//! These DI observations prove ownership and ordering, not physical device timing.
use super::*;
use super::coordinate_pump_fixtures::{button, limits, member};
use crate::{
    audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis},
    bgm::BgmConfig,
    gameplay::output::ports::{
        NativePlaybackObservation, NativeSourcePlaybackQuery, SourcePlaybackFacts,
        SourcePlaybackIdentity,
    },
    gameplay_competition::{NoopGroupCompetition, NoopSoloCompetition},
    gameplay_presentation::GameplayDevice,
    native_audio_presentation::{NativeAudioPresentation, NativeAudioSnapshot},
    native_audio_startup::new_coordinate_audio_presentation,
    native_coordinate_control::CoordinateControlFacts,
    native_gameplay::InputBatch,
    native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate},
    native_playback_phase::NativePlaybackMapping,
    replay_speed::ReplaySpeed,
};
use beatkernel::{
    audio::*,
    input::{ButtonState, DeviceId},
    replay::{ReplayOperation, ReplaySession},
    runtime::{RuntimeProcessingClock, RuntimeReport},
    time::{ClockPair, Duration, ExtrapolationPolicy, presentation::PresentationEstimator},
    transport::{OutputPhaseBasis, Rate, Transport},
};
use beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence;
use std::{cell::Cell, rc::Rc};

#[path = "native_cohort_target_control_fixtures.rs"]
mod target_control_fixtures;

fn cp(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn query(frame: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::from_frames(frame, 3).unwrap(), 0, 0, 1, 0, 1).unwrap()
}
fn observation(basis: OutputFrameBasis, physical: u64, host: i64) -> CoordinateNativeAudioSnapshot {
    CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch: 7,
            basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(physical - 2).unwrap(),
                target: cp(1, host),
            }),
        },
        NativeOutputCoordinate::StreamFrame {
            frame: physical - 2,
        },
    )
    .unwrap()
}

struct Device {
    mixer: Mixer,
    basis: OutputFrameBasis,
    report: RenderReport,
    step: Rc<Cell<usize>>,
    rendered_step: usize,
    missing_at: Option<usize>,
    stale_at: Option<usize>,
    previous: Option<CoordinateControlFacts>,
    events: VecDeque<PhysicalInputEvent>,
    control_calls: usize,
    scalar_calls: usize,
    close_at: usize,
    omit_render_steps: Vec<usize>,
    delayed_pause_report: Option<RenderReport>,
    deliver_delayed_pause_at: Option<usize>,
    prefix_held_until: usize,
    observed_inputs: Vec<(usize, Option<ClockPoint>)>,
}
impl Device {
    fn host(&self) -> i64 {
        104 + 4 * self.step.get() as i64
    }
    fn physical(&self) -> u64 {
        3 + self.step.get() as u64
    }
    fn packet(
        &self,
        q: InterpolatedTargetTime,
        id: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<NativeSourcePlaybackQuery<OriginalNativePresentationEvidence>> {
        let history = self.mixer.source_playback_history().unwrap();
        let playback = history
            .lookup_at_source_time(q)
            .map_err(|e| std::io::Error::other(format!("actual Mixer source span: {e:?}")))?;
        Ok(NativeSourcePlaybackQuery::new(
            7,
            self.basis,
            id,
            q,
            SourcePlaybackFacts {
                playback,
                pulled_source_end: self.physical(),
                submitted_source_end: self.physical(),
                render: if self.omit_render_steps.contains(&self.step.get()) {
                    None
                } else if self.deliver_delayed_pause_at == Some(self.step.get()) {
                    self.delayed_pause_report
                } else {
                    Some(self.report)
                },
                revision: 2 + 2 * self.step.get() as u64,
            },
            Some(
                observation(self.basis, self.physical(), self.host())
                    .snapshot()
                    .evidence,
            ),
        )?)
    }
}
impl GameplayDevice for Device {
    type Presentation = PresentationEstimator;
    fn playback_mapping_at(
        &self,
        _: &NativeAudioPresentation,
        q: &InterpolatedTargetTime,
        id: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<NativePlaybackMapping>> {
        Ok(Some(NativePlaybackMapping::from_source(
            &self.packet(*q, id)?,
        )))
    }
    fn observe_coordinate_control(
        &mut self,
        p: &mut NativeAudioPresentation,
        id: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinateControlFacts>> {
        self.control_calls += 1;
        self.observed_inputs
            .push((self.step.get(), p.authority().committed_input_host()));
        if self.missing_at == Some(self.step.get()) {
            return Ok(None);
        }
        if self.rendered_step < self.step.get() {
            self.report = self.mixer.render(&mut [0.0; 1])?;
            self.rendered_step = self.step.get();
            if self.step.get() == 1 && self.report.paused {
                self.delayed_pause_report = Some(self.report);
            }
        }
        let coordinate = observation(self.basis, self.physical(), self.host());
        p.admit_coordinate(coordinate)?;
        if self.stale_at == Some(self.step.get()) {
            return Ok(self.previous);
        }
        let packet = self.packet(query(self.physical()), id)?;
        let native = NativePlaybackObservation::new(coordinate, packet)?;
        let facts = CoordinateControlFacts::from_source(&native, p, id)?;
        self.previous = Some(facts);
        Ok(Some(facts))
    }
    fn observe(&mut self, _: &mut PresentationEstimator) -> NativeGameplayResult<()> {
        self.scalar_calls += 1;
        Err("scalar observation forbidden in coordinate control entry".into())
    }
    fn observe_audio(&mut self, _: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
        self.scalar_calls += 1;
        Err("independent observation forbidden in compound control entry".into())
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        Ok(Some(self.report))
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(cp(1, self.host()))
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        while self
            .events
            .front()
            .is_some_and(|e| e.meta().timestamp <= cp(1, self.host()).timestamp)
        {
            events.push_back(self.events.pop_front().unwrap());
        }
        let completed = if self.step.get() < self.prefix_held_until {
            102
        } else {
            self.host()
        };
        Ok(InputBatch {
            completed_through: Some(cp(1, completed)),
            backlog: false,
            closed: self.step.get() >= self.close_at,
        })
    }
    fn observe_end(
        &mut self,
        _: &mut NativeEnd,
        _: &PresentationEstimator,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<crate::native_end::EndBoundary>> {
        self.scalar_calls += 1;
        Err("scalar endpoint forbidden".into())
    }
    fn observe_audio_end(
        &mut self,
        _: &mut NativeEnd,
        _: &NativeAudioPresentation,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<crate::native_end::EndBoundary>> {
        self.scalar_calls += 1;
        Err("independent endpoint forbidden".into())
    }
    fn seed_resume(
        &mut self,
        _: &mut PresentationEstimator,
        _: ClockPair,
    ) -> NativeGameplayResult<()> {
        self.scalar_calls += 1;
        Err("coordinate resume cannot reseed".into())
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
struct Control {
    step: Rc<Cell<usize>>,
}
impl NativePumpControl for Control {
    type Moment = usize;
    fn now(&mut self) -> NativeGameplayResult<usize> {
        Ok(self.step.get())
    }
    fn checked_add(moment: usize, d: WallDuration) -> Option<usize> {
        moment.checked_add(usize::try_from(d.as_nanos()).ok()?)
    }
    fn wait(&mut self, _: WallDuration) -> NativeGameplayResult<()> {
        let next = self.step.get() + 1;
        if next > 12 {
            return Err("actual cohort control fixture did not settle".into());
        }
        self.step.set(next);
        Ok(())
    }
}
struct Host {
    step: Rc<Cell<usize>>,
    pause_until: usize,
    reports: Vec<PlayerReport>,
    pauses: Vec<PauseState>,
    fail_reports: bool,
    ends: Vec<Timestamp>,
    completed: usize,
}
impl NativeGameplayHost for Host {
    fn cancelled(&self) -> bool {
        false
    }
    fn pause_requested(&self) -> bool {
        self.step.get() < self.pause_until
    }
    fn retry_pause_publication(&mut self) {}
    fn publish_pause(&mut self, state: PauseState) {
        self.pauses.push(state);
    }
    fn publish_section_end(&mut self, end: Timestamp) {
        self.ends.push(end);
    }
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
    fn publish_report(&mut self, _: &RuntimeReport) -> NativeGameplayResult<()> {
        Err("solo report forbidden".into())
    }
    fn publish_local_reports(&mut self, reports: &[PlayerReport]) -> NativeGameplayResult<()> {
        self.reports.extend_from_slice(reports);
        if self.fail_reports && !reports.is_empty() {
            Err("accepted operation publication failure".into())
        } else {
            Ok(())
        }
    }
    fn publish_completed_local(
        &mut self,
        _: &[(PlayerId, CompletedPlayResult)],
    ) -> NativeGameplayResult<()> {
        self.completed += 1;
        Ok(())
    }
}
struct Fixture {
    group: RuntimeGroup,
    states: Vec<GameplayPlayerState<NoopSoloCompetition>>,
    merger: InputMerger,
    p: NativeAudioPresentation,
    bgm: BgmFeeder,
    pause: NativePause,
    end: Option<NativeEnd>,
    delivery: InputDeliveryTelemetry,
    pre: u64,
    device: Device,
    control: Control,
    host: Host,
    config: CoordinateGameplayConfig,
    speed: LiveSpeedSetup,
    phase: OutputPhaseBasis,
}
impl Fixture {
    fn new(ids: &[u32], rate: Rate, finite: bool) -> Self {
        let step = Rc::new(Cell::new(0));
        let format = AudioFormat::new(3, 1).unwrap();
        let bank = SampleBank::new(format, PcmLimits::new(128, 512, 1).unwrap()).unwrap();
        let (producer, consumer) = command_queue(32).unwrap();
        let speed = LiveSpeedSetup::new(
            Timestamp::ZERO,
            Duration::from_nanos(3),
            ReplaySpeed::new(rate).unwrap(),
            finite.then_some(Timestamp::from_nanos(500_000_000)),
        )
        .unwrap();
        let end_frame = if finite {
            Some(2 + speed.playback_end_frame(3).unwrap().unwrap())
        } else {
            None
        };
        let mut mixer_config = MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(32, 2, 8, 32, 8).unwrap(),
        );
        if let Some(end) = end_frame {
            mixer_config = mixer_config.with_playback_end_frame(end);
        }
        let mut mixer = Mixer::new(mixer_config, bank, consumer).unwrap();
        mixer.enable_source_playback_history(3).unwrap();
        mixer.render(&mut [0.0; 2]).unwrap();
        let report = mixer.render(&mut [0.0; 1]).unwrap();
        let basis = OutputFrameBasis::new(cp(2, 0), 3, 2).unwrap();
        let identity = SourcePlaybackIdentity::new(3, 0, 0).unwrap();
        let mut p = new_coordinate_audio_presentation(
            7,
            AudioAuthorityCoordinateBasis::SourceFrame(basis),
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
        p.admit_coordinate(observation(basis, 2, 100)).unwrap();
        p.admit_coordinate(observation(basis, 3, 104)).unwrap();
        let phase = OutputPhaseBasis::new(ClockDomainId(3), 91, 113, 2, 3).unwrap();
        let transport = Transport::new_with_output_phase(
            phase,
            cp(3, 500),
            speed.start(),
            speed.preroll(),
            rate,
            speed.end(),
        )
        .unwrap();
        let mut states = Vec::new();
        let mut configs = Vec::new();
        for &id in ids {
            let m = member(id);
            let capture = LiveReplayCapture::new_with_gauge_and_speed(
                &m.judge,
                ClockDomainId(3),
                limits(),
                Timestamp::ZERO,
                0,
                speed.end(),
                beatkernel_bms::BmsInputMode::ButtonOnly,
                None,
                &GaugeProfile::default(),
                speed.speed(),
            )
            .unwrap();
            states.push(GameplayPlayerState {
                player: m.player,
                capture: Some(capture),
                competition: None,
                completion: None,
                score: ScoreSummary::default(),
                gauge: BmsGauge::default(),
                last_song: speed.song_origin(),
            });
            configs.push(m);
        }
        let mut group = RuntimeGroup::new(
            ClockDomainId(3),
            ClockDomainId(2),
            transport,
            producer,
            configs,
            8,
            &[],
        )
        .unwrap();
        group.set_processing_clock(RuntimeProcessingClock::Disabled);
        if let Some(end) = speed.end() {
            group.set_song_end(end).unwrap();
        }
        let stream = p.authority().epoch().stream_origin;
        let mut pause = NativePause::new(cp(2, 0), ClockDomainId(1), 3).unwrap();
        if let Some(end) = end_frame {
            pause = pause.with_playback_end_frame(end).unwrap();
        }
        Self {
            group,
            states,
            merger: InputMerger::new(
                ClockDomainId(1),
                cp(1, 100),
                ids.iter().map(|&id| DeviceId(u64::from(id))).collect(),
                32,
            )
            .unwrap(),
            p,
            bgm: BgmFeeder::new(
                vec![],
                BgmConfig {
                    output_origin: cp(2, 0),
                    sample_rate: 3,
                    preroll: speed.preroll(),
                    lookahead: Duration::from_nanos(1_000_000_000),
                    max_pending: 8,
                },
            )
            .unwrap(),
            pause,
            end: end_frame.map(|end| NativeEnd::new(cp(2, 0), ClockDomainId(1), 3, end).unwrap()),
            delivery: InputDeliveryTelemetry::new(8, ClockDomainId(1)).unwrap(),
            pre: 0,
            device: Device {
                mixer,
                basis,
                report,
                step: step.clone(),
                rendered_step: 0,
                missing_at: None,
                stale_at: None,
                previous: None,
                events: VecDeque::new(),
                control_calls: 0,
                scalar_calls: 0,
                close_at: 6,
                omit_render_steps: vec![],
                delayed_pause_report: None,
                deliver_delayed_pause_at: None,
                prefix_held_until: 0,
                observed_inputs: vec![],
            },
            control: Control { step: step.clone() },
            host: Host {
                step,
                pause_until: if finite { 0 } else { 3 },
                reports: vec![],
                pauses: vec![],
                fail_reports: false,
                ends: vec![],
                completed: 0,
            },
            config: CoordinateGameplayConfig {
                audio: AudioGameplayConfig {
                    gameplay: NativeGameplayConfig {
                        origin: cp(1, 100),
                        stream_origin: stream,
                        playback_origin: stream,
                        song_origin: speed.song_origin(),
                        sample_rate: 3,
                        end_song: speed.end(),
                        advance_lag: Duration::ZERO,
                        seconds: None,
                        pause_supported: !finite,
                        logical_schedule: false,
                    },
                    section_start: Timestamp::ZERO,
                },
                source_history: identity,
            },
            speed,
            phase,
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
}

#[test]
fn actual_entry_two_and_four_members_pause_resume_share_one_phase_and_original_cut() {
    for ids in [&[7, 29][..], &[7, 29, 101, 409][..]] {
        for rate in [
            Rate::NORMAL,
            Rate::new(1, 2).unwrap(),
            Rate::new(3, 2).unwrap(),
            Rate::new(2, 1).unwrap(),
        ] {
            let mut f = Fixture::new(ids, rate, false);
            assert!(f.run().unwrap().is_none()); // Genuine input closure, not finite completion.
            assert_eq!(f.device.scalar_calls, 0);
            assert!(f.device.control_calls >= 5);
            assert_eq!(f.pause.phase(), PausePhase::Running);
            assert_eq!(f.group.transport().output_phase_basis(), Some(f.phase));
            assert!(f.host.pauses.contains(&PauseState::Paused));
            assert!(f.host.pauses.contains(&PauseState::Resuming));
            assert_eq!(f.host.completed, 0);
            assert!(f.host.ends.is_empty());
            let songs: Vec<_> = ids
                .iter()
                .map(|&id| {
                    f.host
                        .reports
                        .iter()
                        .filter(|r| r.player == PlayerId(id))
                        .map(|r| r.report.song_time)
                        .collect::<Vec<_>>()
                })
                .collect();
            assert!(!songs[0].is_empty());
            assert!(songs.iter().all(|s| s == &songs[0]));
            let frozen = Timestamp::from_nanos(
                ((333_333_333i128 - 3) * i128::from(rate.numerator())
                    / i128::from(rate.denominator())) as i64,
            );
            assert!(
                songs[0].contains(&frozen),
                "pause original source3 minus active2: {songs:?}"
            );
            let after_resume = Timestamp::from_nanos(
                ((1_000_000_000i128 - 3) * i128::from(rate.numerator())
                    / i128::from(rate.denominator())) as i64,
            );
            assert_eq!(
                songs[0].last(),
                Some(&after_resume),
                "physical8, playback5, active2: the three-frame pause gap is excluded exactly once"
            );
            assert!(f
                .p
                .authority()
                .committed_coordinate_operation()
                .unwrap()
                .compare_interpolated_time(query(8))
                .is_eq());
            for state in &f.states {
                assert_eq!(state.last_song, *songs[0].last().unwrap());
                let capture = state.capture.as_ref().unwrap();
                assert!(capture
                    .records()
                    .iter()
                    .any(|r| matches!(r.operation, ReplayOperation::Advance)));
                let replay = ReplaySession::from_records(
                    capture.header().clone(),
                    member(state.player.0).judge,
                    capture.records().to_vec(),
                )
                .unwrap();
                assert_eq!(
                    replay.engine().stable_hash().unwrap(),
                    f.group
                        .member_judge(state.player)
                        .unwrap()
                        .stable_hash()
                        .unwrap()
                );
            }
            assert!(!f.group.poisoned());
        }
    }
}

#[test]
fn pause_equality_inputs_are_drained_without_judging_and_resume_keeps_member_device_provenance() {
    let mut f = Fixture::new(&[7, 29, 101, 409], Rate::NORMAL, false);
    for id in [409, 101, 29, 7] {
        f.device
            .events
            .push_back(button(id, 104, 1, ButtonState::Down));
    }
    // Release events acquired while paused reconcile the same actual device,
    // rather than introducing a shared synthetic member or a new clock.
    for id in [409, 101, 29, 7] {
        f.device
            .events
            .push_back(button(id, 112, 2, ButtonState::Up));
    }
    assert!(f.run().unwrap().is_none());
    assert_eq!(f.merger.pending(), 0);
    assert_eq!(f.pause.phase(), PausePhase::Running);
    for report in f.host.reports.iter().filter(|r| r.report.input.is_some()) {
        let event = report.report.input.as_ref().unwrap();
        assert_eq!(event.meta().source, DeviceId(u64::from(report.player.0)));
        assert_ne!(event.meta().original_clock_point, Some(cp(1, 104)));
    }
    assert!(f.states.iter().all(|s| s.score.hits == 0));
}

#[test]
fn missing_compound_packet_holds_request_and_never_uses_scalar_observation() {
    let mut f = Fixture::new(&[7, 29], Rate::NORMAL, false);
    f.device.missing_at = Some(0);
    f.device.close_at = 0;
    let before = format!("{:?}", f.p.authority().committed_coordinate_operation());
    assert!(f.run().unwrap().is_none());
    assert_eq!(f.pause.phase(), PausePhase::Running);
    assert_eq!(
        format!("{:?}", f.p.authority().committed_coordinate_operation()),
        before
    );
    assert!(f.host.reports.is_empty());
    assert_eq!(f.device.scalar_calls, 0);
}

#[test]
fn actual_loop_coherent_packet_without_render_retains_original_staged_pause_cut() {
    let mut f = Fixture::new(&[7, 29], Rate::NORMAL, false);
    f.device.prefix_held_until = 3;
    f.device.omit_render_steps = vec![2];
    // Actual pause report at step1 adopts physical3. The acquired prefix is
    // deliberately behind its HOST104 cut until step3; step2 is a legitimate
    // newer original observation with no render report in its publication.
    f.host.pause_until = 5;
    f.device.close_at = 8;
    f.device
        .events
        .push_back(button(7, 103, 1, ButtonState::Down));
    f.device
        .events
        .push_back(button(7, 105, 2, ButtonState::Up));
    assert!(f.run().unwrap().is_none());
    assert_eq!(f.device.scalar_calls, 0);
    assert!(f.host.pauses.contains(&PauseState::Paused));
    let first_input = f
        .host
        .reports
        .iter()
        .find(|r| r.report.input.is_some())
        .unwrap();
    assert_eq!(
        first_input
            .report
            .input
            .as_ref()
            .unwrap()
            .meta()
            .original_clock_point,
        Some(cp(1, 103))
    );
    assert_eq!(
        first_input.report.song_time,
        Timestamp::from_nanos(249_999_997)
    );
    // The first shared pause advance must retain physical3/playback3, rather
    // than a later paused publication's physical frontier4/5.
    let first_advance = f
        .host
        .reports
        .iter()
        .find(|r| r.report.input.is_none())
        .unwrap();
    assert_eq!(
        first_advance.report.song_time,
        Timestamp::from_nanos(333_333_330)
    );
    assert_eq!(
        f.device
            .observed_inputs
            .iter()
            .find(|(step, _)| *step == 3)
            .unwrap()
            .1,
        None
    );
    assert_reconciled_release(&f, 7, 8);
}

#[test]
fn actual_loop_holds_original_input_before_pause_report_then_uses_delayed_genuine_adoption() {
    let mut f = Fixture::new(&[7, 29, 101, 409], Rate::NORMAL, false);
    f.device.omit_render_steps = vec![0, 1];
    f.device.deliver_delayed_pause_at = Some(2);
    f.host.pause_until = 4;
    f.device.close_at = 7;
    f.device
        .events
        .push_back(button(29, 103, 1, ButtonState::Down));
    f.device
        .events
        .push_back(button(29, 105, 2, ButtonState::Up));
    assert!(f.run().unwrap().is_none());
    // At both observations the acquisition prefix already covers these inputs.
    // No original event may be judged while the native owner is still Pausing
    // and the actual adoption receipt/cut has not arrived.
    assert_eq!(
        f.device
            .observed_inputs
            .iter()
            .find(|(step, _)| *step == 1)
            .unwrap()
            .1,
        None
    );
    assert_eq!(
        f.device
            .observed_inputs
            .iter()
            .find(|(step, _)| *step == 2)
            .unwrap()
            .1,
        None
    );
    let original = f.device.delayed_pause_report.unwrap();
    assert!(original.paused);
    assert_eq!(original.start_frame, 3);
    assert_eq!(original.playback_start_frame, 3);
    assert!(f.host.pauses.contains(&PauseState::Paused));
    assert_eq!(f.device.scalar_calls, 0);
    let inputs: Vec<_> = f
        .host
        .reports
        .iter()
        .filter_map(|r| r.report.input.as_ref())
        .collect();
    assert!(inputs
        .iter()
        .any(|e| e.meta().original_clock_point == Some(cp(1, 103))));
    assert_reconciled_release(&f, 29, 7);
    assert_eq!(f.pause.phase(), PausePhase::Running);
}

fn assert_reconciled_release(f: &Fixture, player: u32, physical_resume: u64) {
    let resume_output =
        f.p.authority()
            .epoch()
            .logical_output(
                f.device
                    .basis
                    .point_at_stream_frame(physical_resume - 2)
                    .unwrap(),
            )
            .unwrap();
    let releases: Vec<_> = f
        .host
        .reports
        .iter()
        .filter(|r| {
            r.report
                .input
                .as_ref()
                .is_some_and(|event| event.meta().original_clock_point == Some(cp(1, 105)))
        })
        .collect();
    assert_eq!(releases.len(), 1);
    assert_eq!(releases[0].player, PlayerId(player));
    let release = releases[0].report.input.as_ref().unwrap();
    assert_eq!(release.meta().source, DeviceId(u64::from(player)));
    assert_eq!(release.meta().clock_domain, resume_output.domain);
    assert_eq!(release.meta().timestamp, resume_output.timestamp);
    assert_eq!(
        releases[0].report.song_time,
        Timestamp::from_nanos(333_333_330)
    );
    assert!(matches!(release, PhysicalInputEvent::Button(event) if event.state == ButtonState::Up));
}

#[test]
fn stale_compound_control_publication_cannot_acknowledge_pause_or_commit_boundary() {
    let mut f = Fixture::new(&[7, 29], Rate::NORMAL, false);
    f.device.stale_at = Some(1);
    assert!(f.run().is_err());
    assert_ne!(f.pause.phase(), PausePhase::Paused);
    assert!(!f.host.pauses.contains(&PauseState::Paused));
    assert!(f.host.reports.is_empty());
    assert!(f.p.authority().committed_coordinate_operation().is_none());
    assert!(!f.group.poisoned());
    assert_eq!(f.device.scalar_calls, 0);
}

#[test]
fn fallible_local_publication_preserves_genuine_shared_pause_ack_and_each_report() {
    let mut f = Fixture::new(&[7, 29, 101, 409], Rate::NORMAL, false);
    f.host.fail_reports = true;
    let error = f.run().unwrap_err();
    assert!(error
        .downcast_ref::<NativeCohortObservationError>()
        .is_some());
    assert_eq!(f.pause.phase(), PausePhase::Paused);
    assert!(f.group.transport().is_paused());
    assert!(f.p.authority().committed_coordinate_operation().is_some());
    assert_eq!(f.host.reports.len(), 4);
    assert_eq!(
        f.host.reports.iter().map(|r| r.player).collect::<Vec<_>>(),
        [PlayerId(7), PlayerId(29), PlayerId(101), PlayerId(409)]
    );
    let frozen = Timestamp::from_nanos(333_333_330);
    assert!(f.host.reports.iter().all(|r| r.report.song_time == frozen));
    assert!(f
        .states
        .iter()
        .all(|s| s.capture.as_ref().unwrap().records().len() == 1));
    assert!(!f.group.poisoned());
}

#[test]
fn real_source_end_closes_all_member_phase_captures_before_completed_publication() {
    for ids in [&[7, 29][..], &[7, 29, 101, 409][..]] {
        for rate in [
            Rate::NORMAL,
            Rate::new(1, 2).unwrap(),
            Rate::new(3, 2).unwrap(),
            Rate::new(2, 1).unwrap(),
        ] {
            let mut f = Fixture::new(ids, rate, true);
            f.device.close_at = 12;
            let results = f
                .run()
                .unwrap()
                .expect("actual native endpoint and all members complete");
            assert_eq!(results.len(), ids.len());
            assert_eq!(f.host.completed, 1);
            assert_eq!(f.host.ends, [Timestamp::from_nanos(500_000_000)]);
            assert_eq!(f.device.scalar_calls, 0);
            assert!(f
                .states
                .iter()
                .all(|s| s.last_song == Timestamp::from_nanos(500_000_000)));
            assert!(f.p.authority().committed_coordinate_operation().is_some());
            assert_eq!(f.merger.pending(), 0);
            for state in &f.states {
                let capture = state.capture.as_ref().unwrap();
                assert_eq!(
                    capture.records().last().unwrap().song_time,
                    Timestamp::from_nanos(500_000_000)
                );
                let replay = ReplaySession::from_records(
                    capture.header().clone(),
                    member(state.player.0).judge,
                    capture.records().to_vec(),
                )
                .unwrap();
                assert_eq!(
                    replay.engine().stable_hash().unwrap(),
                    f.group
                        .member_judge(state.player)
                        .unwrap()
                        .stable_hash()
                        .unwrap()
                );
            }
        }
    }
}

// This fault injection enters the SAME private control service used by the real
// loop above. It does not relax the public entry's pristine cold-start gate.
fn control_member_chronology_failure(failed_player: u32) -> Fixture {
    let mut f = Fixture::new(&[7, 29, 101, 409], Rate::NORMAL, false);
    let id = f.config.source_history;
    f.pause = f
        .pause
        .clone()
        .with_coordinate_binding(
            7,
            AudioAuthorityCoordinateBasis::SourceFrame(f.device.basis),
            id,
        )
        .unwrap();
    f.pause.request_coordinate(true, f.p.authority()).unwrap();
    f.group.request_audio_pause(true);
    f.control.step.set(1);
    let facts = f
        .device
        .observe_coordinate_control(&mut f.p, id)
        .unwrap()
        .unwrap();
    let adoption = f
        .pause
        .prepare_coordinate_adoption(facts.source().render, None)
        .unwrap()
        .unwrap();
    assert_eq!(adoption.physical_frame(), 3);
    assert_eq!(adoption.playback_frame(), 3);
    let mut event = button(failed_player, 503, 1, ButtonState::Up);
    event.meta_mut().clock_domain = ClockDomainId(3);
    f.group
        .process_input_output_phase(
            event,
            &ExplicitDomains,
            cp(2, 777),
            f.phase
                .point_from_elapsed_nanos(cp(3, 503), 333_333_334)
                .unwrap(),
        )
        .unwrap();
    f.p.authority_mut()
        .record_acquired_prefix(cp(1, 108))
        .unwrap();
    let cut =
        f.p.authority()
            .presented_coordinate_boundary_host(&adoption.exact_cut(), cp(1, 108))
            .unwrap()
            .unwrap();
    assert_eq!(cut, cp(1, 104));
    let mut controls = CoordinateCohortControls {
        pause: Some(adoption),
        ..Default::default()
    };
    let mut timing = AudioTiming(&mut f.p, Timestamp::ZERO, Some(f.speed), Some(id));
    let mut session: CohortSession<'_, NoopSoloCompetition, NoopGroupCompetition, _> =
        CohortSession {
            group: &mut f.group,
            network: None,
            states: &mut f.states,
            merger: &mut f.merger,
            bgm: &mut f.bgm,
            discipline: &mut timing,
            pause: &mut f.pause,
            end: &mut f.end,
            delivery: &mut f.delivery,
            pre_origin_inputs: &mut f.pre,
        };
    let mut committed = None;
    let error = service_coordinate_cohort_controls(
        &mut f.device,
        &mut session,
        f.config.audio.gameplay,
        cp(1, 108),
        &mut PauseKeyboard::new(),
        &mut None,
        &mut false,
        &mut committed,
        &mut controls,
        Some(cut),
        None,
        &mut None,
        &mut OwnedStopEvidence::default(),
        &mut f.host,
        &mut NativeRenderEvidence::default(),
    )
    .unwrap_err();
    let error = error.downcast_ref::<NativeCohortProcessingError>().unwrap();
    assert_eq!(
        error.group_error.failed_player,
        Some(PlayerId(failed_player))
    );
    if failed_player == 7 {
        assert!(error.group_error.completed_reports.is_empty());
        assert!(committed.is_none());
    } else {
        assert_eq!(error.group_error.completed_reports.len(), 1);
        assert_eq!(error.group_error.completed_reports[0].player, PlayerId(7));
        assert_eq!(committed, Some(cut));
        assert_eq!(
            error.group_error.completed_reports[0].report.song_time,
            Timestamp::from_nanos(333_333_330)
        );
    }
    drop(session);
    drop(timing);
    f
}

#[test]
fn genuine_nonempty_member_control_failure_acknowledges_once_and_retains_original_prefix() {
    let f = control_member_chronology_failure(29);
    assert!(f.group.poisoned());
    assert_eq!(f.pause.phase(), PausePhase::Paused);
    assert!(f.group.transport().is_paused());
    assert!(f.p.authority().committed_coordinate_operation().is_some());
    assert_eq!(f.host.reports.len(), 1);
    assert_eq!(f.host.reports[0].player, PlayerId(7));
    assert_eq!(f.host.reports[0].report.audio_at, cp(2, 777));
    assert_eq!(f.states[0].capture.as_ref().unwrap().records().len(), 1);
    assert!(f.states[1..]
        .iter()
        .all(|s| s.capture.as_ref().unwrap().records().is_empty()));
}

#[test]
fn genuine_empty_member_control_failure_acknowledges_neither_owner_and_restores_transport() {
    let f = control_member_chronology_failure(7);
    assert!(f.group.poisoned());
    assert_eq!(f.pause.phase(), PausePhase::Pausing);
    assert!(!f.group.transport().is_paused());
    assert!(f.p.authority().committed_coordinate_operation().is_none());
    assert!(f.host.reports.is_empty());
    assert!(f
        .states
        .iter()
        .all(|s| s.capture.as_ref().unwrap().records().is_empty()));
}
