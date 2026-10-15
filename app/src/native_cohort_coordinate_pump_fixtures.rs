//! Actual cohort service with mathematical DI packets, not native hardware proof.
use super::*;
use crate::{
    audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis},
    bgm::BgmConfig,
    gameplay_competition::{NoopGroupCompetition, NoopSoloCompetition},
    gameplay_presentation::GameplayDevice,
    gameplay::output::ports::{
        NativeSourcePlaybackQuery, SourcePlaybackFacts, SourcePlaybackIdentity,
    },
    local_runtime::MemberConfig,
    native_audio_presentation::{
        NativeAudioPresentation, NativeAudioSnapshot, TargetNativeAudioSnapshot,
    },
    native_audio_startup::new_coordinate_audio_presentation,
    native_gameplay::InputBatch,
    native_output_coordinate::{
        CoordinateNativeAudioSnapshot, CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate,
    },
    native_playback_phase::{ConvertedPlaybackFacts, NativePlaybackMapping},
};
use beatkernel::{
    audio::*,
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId,
    },
    interaction::PressInstantEvaluator,
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow, Rule},
    replay::{codec::ReplayCodecLimits, ReplayOperation, ReplaySession},
    runtime::{RuntimeProcessingClock, RuntimeReport, SoundBinding},
    time::{presentation::PresentationEstimator, ClockPair, Duration, ExtrapolationPolicy},
    transport::{OutputPhaseBasis, Rate, Transport},
};
use beatkernel_platform::audio::{
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    StreamCounters, presentation::validation::OriginalNativePresentationEvidence,
};
use std::cell::Cell;

fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn at(domain: u32, n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(n),
    }
}
fn identity() -> SourcePlaybackIdentity {
    SourcePlaybackIdentity::new(3, 0, 0).unwrap()
}
fn source_basis() -> OutputFrameBasis {
    OutputFrameBasis::new(at(2, 0), 3, 2).unwrap()
}
fn target_basis() -> TargetFrameBasis {
    TargetFrameBasis::new(
        at(2, 0),
        TargetTime::from_frames(2, 3).unwrap(),
        1_000_000_000,
    )
    .unwrap()
}
fn attempt() -> OutputPhaseBasis {
    OutputPhaseBasis::new(ClockDomainId(3), 91, 113, 1, 3).unwrap()
}
pub(super) fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
pub(super) fn member(player: u32) -> MemberConfig {
    let mut chart = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    chart.objects.push(SourceObject {
        id: ObjectId(1),
        start: Beat::new(499_999_999).unwrap(),
        end: None,
        interaction: InteractionId(1),
        visual: VisualId(0),
        audio: None,
        metadata: ObjectMetadata::default(),
    });
    MemberConfig {
        player: PlayerId(player),
        device: Some(DeviceId(u64::from(player))),
        bindings: BindingMap::from_bindings([Binding {
            device: DeviceSelector::Exact(DeviceId(u64::from(player))),
            physical: PhysicalControlId::keyboard(4),
            game_control: GameControlId(1),
        }])
        .unwrap(),
        judge: JudgeEngine::new(
            chart.compile().unwrap(),
            vec![Rule {
                interaction: InteractionId(1),
                control: GameControlId(1),
                evaluator: Box::new(PressInstantEvaluator),
            }],
            JudgeProfile::new(
                vec![JudgeWindow {
                    grade: JudgeGrade(1),
                    early: Duration::ZERO,
                    late: Duration::ZERO,
                }],
                Duration::ZERO,
            )
            .unwrap(),
        )
        .unwrap(),
        sounds: vec![SoundBinding {
            object: ObjectId(1),
            stage: JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(u64::from(player)),
            gain: 0.5,
        }],
    }
}
pub(super) fn button(
    player: u32,
    host: i64,
    sequence: u64,
    state: ButtonState,
) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(u64::from(player)), at(1, host), sequence),
        control: PhysicalControlId::keyboard(4),
        state,
    })
}
fn admit(p: &mut NativeAudioPresentation, target: bool, unit: u64, host: i64) {
    if target {
        let b = target_basis();
        p.admit_target_coordinate(
            CoordinateTargetNativeAudioSnapshot::new(
                TargetNativeAudioSnapshot {
                    epoch: 7,
                    basis: b,
                    evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                        source: b.point_at_stream_frame(unit).unwrap(),
                        target: at(1, host),
                    }),
                },
                NativeOutputCoordinate::StreamFrame { frame: unit },
            )
            .unwrap(),
        )
        .unwrap();
    } else {
        let b = source_basis();
        p.admit_coordinate(
            CoordinateNativeAudioSnapshot::new(
                NativeAudioSnapshot {
                    epoch: 7,
                    basis: b,
                    evidence: OriginalNativePresentationEvidence::Wasapi {
                        basis: Some(b),
                        snapshot: AudioStreamSnapshot {
                            telemetry_available: true,
                            status: AudioStreamStatus::Running,
                            counters: StreamCounters::default(),
                            render: None,
                            clock: Some(AudioClockSnapshot {
                                position: unit,
                                frequency: 1_000_000_000,
                                qpc_100ns: unit,
                                reading_quality: AudioClockReadingQuality::Accurate,
                                host_point: Some(at(1, host)),
                                mapping_quality: ClockMappingQuality::Unknown,
                            }),
                        },
                    },
                },
                NativeOutputCoordinate::Counter {
                    position: unit,
                    frequency: 1_000_000_000,
                },
            )
            .unwrap(),
        )
        .unwrap();
    }
}
struct Device {
    source: Mixer,
    target: bool,
    unavailable: bool,
    suspended: bool,
    calls: Cell<usize>,
}
impl GameplayDevice for Device {
    type Presentation = PresentationEstimator;
    fn playback_mapping_at(
        &self,
        _: &NativeAudioPresentation,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<NativePlaybackMapping>> {
        self.calls.set(self.calls.get() + 1);
        if self.unavailable {
            return Ok(None);
        }
        let history = self.source.source_playback_history().unwrap();
        let playback = history.lookup_at_source_time(*query).map_err(|error| {
            std::io::Error::other(format!("actual source-history query refused: {error:?}"))
        })?;
        if self.target {
            return Ok(Some(NativePlaybackMapping::converted(
                7,
                target_basis(),
                identity,
                *query,
                ConvertedPlaybackFacts {
                    // Exact source2 at target2/3, both active at the original source rate.
                    phase: ConvertedPhaseSpan::from_words([3, 2, 0, 1, 0, 2, 3, 0]).unwrap(),
                    segment_end: TargetTime::from_frames(8, 3).unwrap(),
                    generated_target_end: TargetTime::from_frames(8, 3).unwrap(),
                    submitted_target_end: TargetTime::from_frames(8, 3).unwrap(),
                    playback,
                    pulled_source_end: 8,
                    revision: 2,
                },
            )?));
        }
        let packet: NativeSourcePlaybackQuery<()> = NativeSourcePlaybackQuery::new(
            7,
            source_basis(),
            identity,
            *query,
            SourcePlaybackFacts {
                playback,
                pulled_source_end: 8,
                submitted_source_end: 8,
                render: None,
                revision: 2,
            },
            None,
        )?;
        Ok(Some(NativePlaybackMapping::from_source(&packet)))
    }
    fn output_clock_suspended(&self) -> bool {
        self.suspended
    }
    fn observe(&mut self, _: &mut Self::Presentation) -> NativeGameplayResult<()> {
        Err("scalar route forbidden".into())
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        Ok(None)
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(at(1, 108))
    }
    fn acquire(
        &mut self,
        _: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        Err("service has acquired prefix".into())
    }
    fn observe_end(
        &mut self,
        _: &mut NativeEnd,
        _: &Self::Presentation,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<crate::native_end::EndBoundary>> {
        Err("scalar end forbidden".into())
    }
    fn seed_resume(
        &mut self,
        _: &mut Self::Presentation,
        _: ClockPair,
    ) -> NativeGameplayResult<()> {
        Err("reseed forbidden".into())
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        Ok(at(2, 777))
    }
}
#[derive(Default)]
struct Host {
    reports: Vec<PlayerReport>,
}
impl NativeGameplayHost for Host {
    fn cancelled(&self) -> bool {
        false
    }
    fn pause_requested(&self) -> bool {
        false
    }
    fn retry_pause_publication(&mut self) {}
    fn publish_pause(&mut self, _: PauseState) {}
    fn publish_section_end(&mut self, _: Timestamp) {}
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
    fn publish_report(&mut self, _: &RuntimeReport) -> NativeGameplayResult<()> {
        Err("solo publication forbidden".into())
    }
    fn publish_local_reports(&mut self, reports: &[PlayerReport]) -> NativeGameplayResult<()> {
        self.reports.extend_from_slice(reports);
        Ok(())
    }
}
struct Fixture {
    group: RuntimeGroup,
    queue: CommandConsumer,
    states: Vec<GameplayPlayerState<NoopSoloCompetition>>,
    merger: InputMerger,
    presentation: NativeAudioPresentation,
    bgm: BgmFeeder,
    pause: NativePause,
    end: Option<NativeEnd>,
    delivery: InputDeliveryTelemetry,
    preorigin: u64,
    config: NativeGameplayConfig,
    device: Device,
    host: Host,
    committed: Option<ClockPoint>,
}
impl Fixture {
    fn new(ids: &[u32], target: bool) -> Self {
        let format = AudioFormat::new(3, 1).unwrap();
        let bank = SampleBank::new(format, PcmLimits::new(128, 512, 1).unwrap()).unwrap();
        let (source_producer, source_consumer) = command_queue(16).unwrap();
        let mut source = Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(2),
                ts(0),
                AudioLimits::new(16, 2, 8, 16, 8).unwrap(),
            ),
            bank,
            source_consumer,
        )
        .unwrap();
        drop(source_producer);
        source.enable_source_playback_history(3).unwrap();
        source.render(&mut [0.0; 8]).unwrap();
        let descriptor = if target {
            AudioAuthorityCoordinateBasis::TargetFrame(target_basis())
        } else {
            AudioAuthorityCoordinateBasis::SourceCounter {
                basis: source_basis(),
                frequency: 1_000_000_000,
            }
        };
        let mut presentation = new_coordinate_audio_presentation(
            7,
            descriptor,
            ClockDomainId(1),
            at(3, 500),
            AudioAuthorityConfig {
                history_capacity: 8,
                max_observation_age: Duration::from_nanos(100),
                input_extrapolation: ExtrapolationPolicy::Bounded {
                    before: Duration::from_nanos(10),
                    after: Duration::from_nanos(10),
                },
                max_input_ahead: Duration::from_nanos(10),
            },
        )
        .unwrap();
        if target {
            admit(&mut presentation, true, 0, 100);
            admit(&mut presentation, true, 1, 104);
        } else {
            // WASAPI counter zero is unavailable. These genuine positive-unit
            // anchors define the SAME query line: counter1 at HOST104,
            // counter2 at108, so HOST102/103 select counter1/2 and3/4.
            admit(&mut presentation, false, 1, 104);
            admit(&mut presentation, false, 2, 108);
        }
        presentation
            .authority_mut()
            .record_acquired_prefix(at(1, 104))
            .unwrap();
        let mut configs = Vec::new();
        let mut states = Vec::new();
        for &id in ids {
            let m = member(id);
            states.push(GameplayPlayerState {
                player: m.player,
                capture: Some(
                    LiveReplayCapture::new(&m.judge, ClockDomainId(3), limits()).unwrap(),
                ),
                competition: None,
                completion: None,
                score: ScoreSummary::default(),
                gauge: BmsGauge::default(),
                last_song: ts(0),
            });
            configs.push(m);
        }
        let (producer, queue) = command_queue(16).unwrap();
        let transport = Transport::new_with_output_phase(
            attempt(),
            at(3, 500),
            ts(0),
            Duration::ZERO,
            Rate::new(3, 2).unwrap(),
            None,
        )
        .unwrap();
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
        let stream = presentation.authority().epoch().stream_origin;
        Self {
            group,
            queue,
            states,
            merger: InputMerger::new(
                ClockDomainId(1),
                at(1, 0),
                ids.iter().map(|&n| DeviceId(u64::from(n))).collect(),
                32,
            )
            .unwrap(),
            presentation,
            bgm: BgmFeeder::new(
                vec![],
                BgmConfig {
                    output_origin: at(2, 0),
                    sample_rate: 3,
                    preroll: Duration::ZERO,
                    lookahead: Duration::from_nanos(1_000_000_000),
                    max_pending: 8,
                },
            )
            .unwrap(),
            pause: NativePause::new(at(2, 0), ClockDomainId(1), 3).unwrap(),
            end: None,
            delivery: InputDeliveryTelemetry::new(8, ClockDomainId(1)).unwrap(),
            preorigin: 0,
            config: NativeGameplayConfig {
                origin: at(1, 100),
                stream_origin: stream,
                playback_origin: stream,
                song_origin: ts(0),
                sample_rate: 3,
                end_song: None,
                advance_lag: Duration::ZERO,
                seconds: None,
                pause_supported: false,
                logical_schedule: false,
            },
            device: Device {
                source,
                target,
                unavailable: false,
                suspended: false,
                calls: Cell::new(0),
            },
            host: Host::default(),
            committed: None,
        }
    }
    fn admit_input(&mut self, id: u32, host: i64, sequence: u64, state: ButtonState) {
        self.merger
            .admit(button(id, host, sequence, state), at(1, 104))
            .unwrap();
    }
    fn service(&mut self, practice_end: Option<Timestamp>) -> NativeGameplayResult<()> {
        let mut timing = AudioTiming(&mut self.presentation, ts(0), None, Some(identity()));
        let mut session: CohortSession<'_, NoopSoloCompetition, NoopGroupCompetition, _> =
            CohortSession {
                group: &mut self.group,
                network: None,
                states: &mut self.states,
                merger: &mut self.merger,
                bgm: &mut self.bgm,
                discipline: &mut timing,
                pause: &mut self.pause,
                end: &mut self.end,
                delivery: &mut self.delivery,
                pre_origin_inputs: &mut self.preorigin,
            };
        service_audio_cohort(
            &mut self.device,
            &mut session,
            &mut self.config,
            at(1, 108),
            false,
            &mut PauseKeyboard::new(),
            &mut None,
            &mut None,
            &mut false,
            &mut self.committed,
            false,
            None,
            None,
            &mut None,
            &mut OwnedStopEvidence::default(),
            &mut self.host,
            None,
            None,
            practice_end,
            &mut NativeRenderEvidence::default(),
        )
    }
}

#[test]
fn actual_source_and_target_cohort_service_preserves_fractional_phase_and_capture() {
    for target in [false, true] {
        for ids in [&[7, 29][..], &[7, 29, 101, 409][..]] {
            let mut f = Fixture::new(ids, target);
            for &id in ids.iter().rev() {
                f.admit_input(id, 102, 1, ButtonState::Down);
                f.admit_input(id, 103, 2, ButtonState::Up);
            }
            let a = f
                .presentation
                .authority()
                .prepare_coordinate_input(at(1, 102), at(1, 104))
                .unwrap()
                .unwrap();
            let b = f
                .presentation
                .authority()
                .prepare_coordinate_input(at(1, 103), at(1, 104))
                .unwrap()
                .unwrap();
            assert_eq!(a.raw_output(), b.raw_output());
            assert!(!a.query().compare_interpolated_time(b.query()).is_eq());
            f.service(None).unwrap();
            assert_eq!(f.merger.pending(), 0);
            assert_eq!(
                f.presentation.authority().committed_input_host(),
                Some(at(1, 103))
            );
            assert_eq!(
                f.presentation.authority().closed_host_prefix(),
                Some(at(1, 104))
            );
            for &id in ids {
                let reports: Vec<_> = f
                    .host
                    .reports
                    .iter()
                    .filter(|r| r.player == PlayerId(id))
                    .collect();
                assert_eq!(
                    reports
                        .iter()
                        .map(|r| r.report.song_time.as_nanos())
                        .collect::<Vec<_>>(),
                    [499_999_999, 500_000_001, 500_000_001]
                );
                assert_eq!(reports[0].report.audio_at, at(2, 777));
                assert_eq!(
                    reports[0]
                        .report
                        .input
                        .as_ref()
                        .unwrap()
                        .meta()
                        .original_clock_point,
                    Some(at(1, 102))
                );
                let state = f.states.iter().find(|s| s.player == PlayerId(id)).unwrap();
                assert_eq!(state.score.hits, 1);
                let capture = state.capture.as_ref().unwrap();
                let recorded: Vec<_> = capture
                    .records()
                    .iter()
                    .filter_map(|r| match &r.operation {
                        ReplayOperation::Input(input) => {
                            Some((r.song_time, input.physical.meta().original_clock_point))
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    recorded,
                    [
                        (ts(499_999_999), Some(at(1, 102))),
                        (ts(500_000_001), Some(at(1, 103)))
                    ]
                );
                let file = beatkernel::replay::codec::ReplayFile::new(
                    capture.header().clone(),
                    capture.records().to_vec(),
                );
                let encoded = beatkernel::replay::codec::encode_replay(&file, limits()).unwrap();
                let decoded = beatkernel::replay::codec::decode_replay(&encoded, limits()).unwrap();
                let replay =
                    ReplaySession::from_records(decoded.header, member(id).judge, decoded.records)
                        .unwrap();
                assert_eq!(
                    replay.engine().stable_hash().unwrap(),
                    f.group
                        .member_judge(PlayerId(id))
                        .unwrap()
                        .stable_hash()
                        .unwrap()
                );
            }
            for &id in ids {
                assert_eq!(
                    f.queue.try_pop().unwrap(),
                    AudioCommand::Play {
                        voice: VoiceId(u64::from(id)),
                        sample: SampleId(1),
                        at: ts(777),
                        gain: 0.5
                    }
                );
            }
            assert_eq!(f.queue.try_pop(), Err(QueuePopError::Empty));
            assert!(!f.group.poisoned());
        }
    }
}

#[test]
fn missing_mapping_holds_original_input_and_both_authority_watermarks() {
    let mut f = Fixture::new(&[7, 29], false);
    f.device.unavailable = true;
    f.admit_input(7, 102, 1, ButtonState::Down);
    let original = f.merger.peek_ready(at(1, 104)).unwrap().unwrap().clone();
    let before = format!("{:?}", f.presentation);
    f.service(None).unwrap();
    assert_eq!(f.merger.pending(), 1);
    assert_eq!(f.merger.peek_ready(at(1, 104)).unwrap().unwrap(), &original);
    assert_eq!(format!("{:?}", f.presentation), before);
    assert!(f.host.reports.is_empty());
    assert!(!f.group.poisoned());
    assert_eq!(f.queue.try_pop(), Err(QueuePopError::Empty));
    f.device.unavailable = false;
    f.service(None).unwrap();
    assert_eq!(f.merger.pending(), 0);
    assert_eq!(f.states[0].score.hits, 1);
}

#[test]
fn suspended_output_preserves_acquired_input_and_does_not_request_mapping() {
    let mut f = Fixture::new(&[7, 29], true);
    f.device.suspended = true;
    f.admit_input(7, 102, 1, ButtonState::Down);
    let before = format!("{:?}", f.presentation);
    f.service(None).unwrap();
    assert_eq!(f.merger.pending(), 1);
    assert_eq!(f.device.calls.get(), 0);
    assert_eq!(format!("{:?}", f.presentation), before);
    assert!(f.host.reports.is_empty());
    assert!(!f.group.poisoned());
    assert_eq!(f.queue.try_pop(), Err(QueuePopError::Empty));
}

#[test]
fn exact_preorigin_retires_original_host_without_gameplay_input_or_operation_commit() {
    let mut f = Fixture::new(&[7, 29], false);
    f.device.unavailable = true;
    f.admit_input(7, 99, 1, ButtonState::Down);
    let token = f
        .presentation
        .authority()
        .prepare_coordinate_input(at(1, 99), at(1, 104))
        .unwrap()
        .unwrap();
    assert!(token.is_before_origin());
    f.service(None).unwrap();
    assert_eq!(f.preorigin, 1);
    assert_eq!(f.merger.pending(), 0);
    assert_eq!(
        f.presentation.authority().committed_input_host(),
        Some(at(1, 99))
    );
    assert!(f
        .presentation
        .authority()
        .committed_coordinate_operation()
        .is_none());
    assert!(f.host.reports.is_empty());
    assert!(f.states.iter().all(|s| s.last_song == ts(0)));
    assert_eq!(f.queue.try_pop(), Err(QueuePopError::Empty));
}

#[test]
fn finite_phase_end_holds_equal_input_before_original_queue_consumption() {
    let mut f = Fixture::new(&[7, 29], false);
    f.admit_input(7, 102, 1, ButtonState::Down);
    let before = format!("{:?}", f.presentation);
    f.service(Some(ts(499_999_999))).unwrap();
    assert_eq!(f.merger.pending(), 1);
    assert_eq!(format!("{:?}", f.presentation), before);
    assert!(f.host.reports.is_empty());
    assert!(!f.group.poisoned());
    assert_eq!(f.queue.try_pop(), Err(QueuePopError::Empty));
}

fn seed_member_later(f: &mut Fixture) {
    // Actual public Runtime operation establishes member-only chronology, not
    // fake native observations or an authority commitment. Its queue is retained.
    let mut event = button(29, 503, 1, ButtonState::Up);
    event.meta_mut().clock_domain = ClockDomainId(3);
    f.group
        .process_input_output_phase(
            event,
            &ExplicitDomains,
            at(2, 777),
            attempt()
                .point_from_elapsed_nanos(at(3, 503), 333_333_336)
                .unwrap(),
        )
        .unwrap();
}

#[test]
fn actual_nonempty_second_member_failure_commits_frontier_and_publishes_prefix_once() {
    let mut f = Fixture::new(&[7, 29, 101, 409], false);
    seed_member_later(&mut f);
    assert!(f
        .presentation
        .authority()
        .committed_coordinate_operation()
        .is_none());
    let error = f.service(None).unwrap_err();
    let error = error.downcast_ref::<NativeCohortProcessingError>().unwrap();
    assert_eq!(error.group_error.failed_player, Some(PlayerId(29)));
    assert_eq!(error.group_error.completed_reports.len(), 1);
    assert_eq!(error.group_error.completed_reports[0].player, PlayerId(7));
    assert!(f.group.poisoned());
    assert_eq!(
        f.presentation.authority().closed_host_prefix(),
        Some(at(1, 104))
    );
    assert!(f
        .presentation
        .authority()
        .committed_coordinate_presentation()
        .is_some());
    assert!(f
        .presentation
        .authority()
        .committed_coordinate_operation()
        .is_some());
    assert_eq!(f.host.reports.len(), 1);
    assert_eq!(f.host.reports[0].player, PlayerId(7));
    assert!(f.states[0]
        .capture
        .as_ref()
        .unwrap()
        .records()
        .iter()
        .any(|r| matches!(r.operation, ReplayOperation::Advance)));
    assert!(f.states[2].capture.as_ref().unwrap().records().is_empty());
    assert!(f.states[3].capture.as_ref().unwrap().records().is_empty());
}

#[test]
fn actual_empty_member_failure_does_not_commit_coordinate_input_or_publish_reports() {
    let mut f = Fixture::new(&[7, 29], false);
    seed_member_later(&mut f);
    f.admit_input(29, 102, 2, ButtonState::Down);
    let before = format!("{:?}", f.presentation);
    let error = f.service(None).unwrap_err();
    let error = error.downcast_ref::<NativeCohortProcessingError>().unwrap();
    assert!(error.group_error.completed_reports.is_empty());
    assert_eq!(error.group_error.failed_player, Some(PlayerId(29)));
    assert_eq!(format!("{:?}", f.presentation), before);
    assert!(f.host.reports.is_empty());
    assert!(f.group.poisoned());
}

#[test]
fn actual_reported_queue_failure_commits_input_and_captures_completed_member() {
    let mut f = Fixture::new(&[7, 29], false);
    for i in 0..16 {
        f.group
            .enqueue_audio(AudioCommand::Stop {
                voice: VoiceId(10_000 + i),
                at: ts(777),
            })
            .unwrap();
    }
    f.admit_input(29, 102, 1, ButtonState::Down);
    let error = f.service(None).unwrap_err();
    let error = error.downcast_ref::<NativeCohortProcessingError>().unwrap();
    assert!(matches!(
        error.group_error.kind,
        crate::local_runtime::FailureKind::ReportedFailure
    ));
    assert_eq!(error.group_error.completed_reports.len(), 1);
    assert_eq!(error.group_error.completed_reports[0].player, PlayerId(29));
    assert_eq!(
        error.group_error.completed_reports[0]
            .report
            .audio_failures
            .len(),
        1
    );
    assert_eq!(
        error.group_error.completed_reports[0]
            .report
            .judge_events
            .len(),
        1
    );
    assert_eq!(
        f.presentation.authority().committed_input_host(),
        Some(at(1, 102))
    );
    assert!(f
        .presentation
        .authority()
        .committed_coordinate_operation()
        .is_some());
    assert!(f.presentation.authority().closed_host_prefix().is_none());
    assert_eq!(f.merger.pending(), 0);
    assert!(f.group.poisoned());
    assert_eq!(f.host.reports.len(), 1);
    assert_eq!(f.states[1].score.hits, 1);
    assert!(f.states[1]
        .capture
        .as_ref()
        .unwrap()
        .records()
        .iter()
        .any(
            |r| matches!(r.operation, ReplayOperation::Input(_)) && r.song_time == ts(499_999_999)
        ));
    for i in 0..16 {
        assert_eq!(
            f.queue.try_pop().unwrap(),
            AudioCommand::Stop {
                voice: VoiceId(10_000 + i),
                at: ts(777)
            }
        );
    }
    assert_eq!(f.queue.try_pop(), Err(QueuePopError::Empty));
}
