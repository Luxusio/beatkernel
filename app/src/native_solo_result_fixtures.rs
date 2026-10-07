//! Deferred actual completion classification using portable pumps and Mixer.
use crate::gameplay_presentation_port_fixtures::*;
use crate::{
    completion::SongCompletion,
    local_runtime::SoloRuntime,
    native_gameplay::{GameplaySession, run_gameplay_with_result_and_ports},
    play_result::{
        CompletedPlayResult, CompletedSoloPublicationError, PlayResultOutcome, PlayResultScope,
    },
};

#[derive(Debug)]
pub(crate) struct PublicationRefused;
impl std::fmt::Display for PublicationRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("completed publication refused")
    }
}
impl std::error::Error for PublicationRefused {}
pub(crate) type ResultTrace = std::rc::Rc<std::cell::RefCell<Vec<&'static str>>>;
#[derive(Default)]
pub(crate) struct ResultObserver {
    pub marks: usize,
    pub trace: ResultTrace,
    pub label: &'static str,
}
impl SoloCompetitionPort for ResultObserver {
    fn observe(&mut self, _: &RuntimeReport) -> NativeGameplayResult<()> {
        Ok(())
    }
    fn mark_native_completed(&mut self) {
        self.marks += 1;
        self.trace.borrow_mut().push(self.label);
    }
}
#[derive(Default)]
pub(crate) struct ResultHost {
    pub base: Host,
    pub results: Vec<CompletedPlayResult>,
    pub tables: Vec<Vec<(crate::local_players::PlayerId, CompletedPlayResult)>>,
    pub reject: bool,
    pub cancel_at: Option<Timestamp>,
    pub trace: ResultTrace,
}
impl NativeGameplayHost for ResultHost {
    fn cancelled(&self) -> bool {
        self.base.cancel
    }
    fn pause_requested(&self) -> bool {
        false
    }
    fn retry_pause_publication(&mut self) {}
    fn publish_pause(&mut self, state: PauseState) {
        self.base.publish_pause(state);
    }
    fn publish_section_end(&mut self, at: Timestamp) {
        self.base.publish_section_end(at);
    }
    fn publish_report(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
        if self.cancel_at.is_some_and(|at| report.song_time >= at) {
            self.base.cancel = true;
        }
        self.base.publish_report(report)
    }
    fn publish_local_reports(
        &mut self,
        reports: &[crate::local_runtime::PlayerReport],
    ) -> NativeGameplayResult<()> {
        if self
            .cancel_at
            .is_some_and(|at| reports.iter().any(|r| r.report.song_time >= at))
        {
            self.base.cancel = true;
        }
        self.base.publish_local_reports(reports)
    }
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
    fn publish_completed_solo(&mut self, result: CompletedPlayResult) -> NativeGameplayResult<()> {
        self.results.push(result);
        self.trace.borrow_mut().push("solo-result");
        if self.reject {
            Err(Box::new(PublicationRefused))
        } else {
            Ok(())
        }
    }
    fn publish_completed_local(
        &mut self,
        results: &[(crate::local_players::PlayerId, CompletedPlayResult)],
    ) -> NativeGameplayResult<()> {
        self.tables.push(results.to_vec());
        self.trace.borrow_mut().push("local-result");
        if self.reject {
            Err(Box::new(PublicationRefused))
        } else {
            Ok(())
        }
    }
}
pub(crate) fn result_source(fatal: bool) -> beatkernel_bms::BmsChart {
    beatkernel_bms::parse(
        if fatal {
            "#BPM 3000\n#WAV01 original.wav\n#00011:00010000\n#000D1:00ZZ0000\n"
        } else {
            "#BPM 3000\n#WAV01 original.wav\n#00011:00010000\n"
        },
        Default::default(),
    )
    .unwrap()
}
pub(crate) fn result_judge(source: &beatkernel_bms::BmsChart, fatal: bool) -> JudgeEngine {
    let mut judge = judge(source);
    if fatal {
        use beatkernel::judge::{HazardId, HazardMarker, HazardTimeline};
        judge
            .configure_hazards(
                HazardTimeline::new(
                    vec![HazardMarker {
                        id: HazardId(1),
                        at: Timestamp::from_nanos(20_000_000),
                        control: GameControlId(0x11),
                        value: 1295,
                    }],
                    1,
                )
                .unwrap(),
            )
            .unwrap();
    }
    judge
}
pub(crate) fn completion(source: &beatkernel_bms::BmsChart) -> SongCompletion {
    let format = AudioFormat::new(1000, 1).unwrap();
    let prepared = crate::PreparedBms {
        source: source.clone(),
        compiled: source.compile().unwrap(),
        bank: SampleBank::new(format, PcmLimits::new(64, 256, 1).unwrap()).unwrap(),
        sounds: vec![],
        bgm_commands: vec![],
    };
    SongCompletion::prepare(&prepared, 0, 0, 0, ClockDomainId(2)).unwrap()
}
pub(crate) fn cleared_gauge() -> BmsGauge {
    use beatkernel::{
        chart::ObjectId,
        judge::{JudgeEvent, JudgeOutcome, JudgeStage},
    };
    let mut gauge = BmsGauge::default();
    // Inject a known previously observed prefix, never alter the gauge policy.
    let events = (1..=60)
        .map(|id| JudgeEvent {
            object: ObjectId(id),
            stage: JudgeStage::Instant,
            outcome: JudgeOutcome::Hit {
                grade: JudgeGrade(1),
                delta: Duration::ZERO,
            },
            at: Timestamp::ZERO,
            input: None,
        })
        .collect::<Vec<_>>();
    gauge.observe(&events, &[]).unwrap();
    gauge
}
struct Fixture {
    device: MemoryDevice,
    runtime: SoloRuntime,
    gauge: BmsGauge,
    bgm: BgmFeeder,
    presentation: PresentationEstimator,
    pause: NativePause,
    end: Option<NativeEnd>,
    completion: Option<SongCompletion>,
    capture: Option<crate::replay_capture::LiveReplayCapture>,
    observer: Option<ResultObserver>,
    delivery: InputDeliveryTelemetry,
    pre: u64,
}
impl Fixture {
    fn new(finite: bool, fatal: bool) -> Self {
        let source = result_source(fatal);
        let (mut device, producer) = device(finite, vec![DeviceId(u64::MAX)]);
        device.close = false;
        let mut runtime = SoloRuntime::new(
            ClockDomainId(1),
            ClockDomainId(2),
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
            bindings(None),
            result_judge(&source, fatal),
            producer,
            vec![],
            8,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        if finite {
            runtime
                .set_song_end(Timestamp::from_nanos(10_000_000))
                .unwrap();
        }
        let (pause, end) = pause_end(finite);
        Self {
            device,
            runtime,
            gauge: BmsGauge::default(),
            bgm: bgm(),
            presentation: estimator(),
            pause,
            end,
            completion: (!finite).then(|| completion(&source)),
            capture: None,
            observer: Some(ResultObserver {
                label: "solo-mark",
                ..Default::default()
            }),
            delivery: InputDeliveryTelemetry::new(16, ClockDomainId(1)).unwrap(),
            pre: 0,
        }
    }
    fn run<C: NativePumpControl>(
        &mut self,
        config: NativeGameplayConfig,
        host: &mut ResultHost,
        control: &mut C,
    ) -> NativeGameplayResult<Option<CompletedPlayResult>> {
        host.trace = self.observer.as_ref().unwrap().trace.clone();
        run_gameplay_with_result_and_ports(
            &mut self.device,
            GameplaySession {
                runtime: &mut self.runtime,
                gauge: &mut self.gauge,
                bgm: &mut self.bgm,
                discipline: &mut self.presentation,
                pause: &mut self.pause,
                end: &mut self.end,
                completion: &mut self.completion,
                capture: &mut self.capture,
                competition: &mut self.observer,
                delivery: &mut self.delivery,
                pre_origin_inputs: &mut self.pre,
            },
            config,
            control,
            host,
        )
    }
}

#[test]
fn solo_real_completion_classifies_original_scope_and_exact_actual_gauge() {
    for (finite, cleared, start) in [
        (true, false, 0),
        (true, true, 0),
        (false, false, 0),
        (false, true, 0),
        (false, false, 1),
    ] {
        let mut f = Fixture::new(finite, false);
        if cleared {
            f.gauge = cleared_gauge();
        }
        let mut config = config(finite);
        config.song_origin = Timestamp::from_nanos(start);
        if start != 0 {
            *f.runtime.transport_mut() =
                Transport::new(Timestamp::ZERO, Timestamp::from_nanos(start), Rate::NORMAL);
        }
        let mut host = ResultHost::default();
        let result = f
            .run(config, &mut host, &mut Control::default())
            .unwrap()
            .unwrap();
        assert_eq!(
            result.scope(),
            if finite || start != 0 {
                PlayResultScope::PracticeSection {
                    start: Timestamp::from_nanos(start),
                    end: config.end_song,
                }
            } else {
                PlayResultScope::FullSong
            }
        );
        assert_eq!(
            result.outcome(),
            if cleared {
                PlayResultOutcome::Cleared
            } else {
                PlayResultOutcome::BelowClearThreshold
            }
        );
        assert_eq!(result.gauge(), *f.gauge.snapshot());
        assert_eq!(result.whole_song_clear(), cleared && !finite && start == 0);
        assert_eq!(host.results, [result]);
        assert_eq!(f.observer.as_ref().unwrap().marks, 1);
        assert_eq!(&*host.trace.borrow(), &["solo-mark", "solo-result"]);
        assert!(f.device.step >= 3);
        assert!(f.device.report.unwrap().pending_commands == 0);
    }
    let mut f = Fixture::new(false, true);
    let mut host = ResultHost::default();
    let result = f
        .run(config(false), &mut host, &mut Control::default())
        .unwrap()
        .unwrap();
    assert_eq!(
        result.outcome(),
        PlayResultOutcome::Failed(crate::gauge::GaugeFailure::InstantDeath)
    );
    assert_eq!(result.gauge().level_units, 0);
    assert_eq!(
        f.runtime.gameplay_fence(),
        Some(Timestamp::from_nanos(20_000_000))
    );
    assert!(
        host.base
            .solo
            .iter()
            .any(|r| r.hazard_events.iter().any(|h| h.value == 1295))
    );
    assert_eq!(host.results, [result]);
    assert_eq!(f.observer.as_ref().unwrap().marks, 1);
    assert_eq!(&*host.trace.borrow(), &["solo-mark", "solo-result"]);
}

#[test]
fn solo_publication_refusal_carries_exact_proven_result_and_original_cause() {
    let mut f = Fixture::new(true, false);
    let mut host = ResultHost {
        reject: true,
        ..Default::default()
    };
    let error = f
        .run(config(true), &mut host, &mut Control::default())
        .unwrap_err();
    let refusal = error
        .downcast_ref::<CompletedSoloPublicationError>()
        .unwrap();
    assert!(refusal.cause.downcast_ref::<PublicationRefused>().is_some());
    assert_eq!(host.results, [refusal.result]);
    assert_eq!(refusal.result.gauge(), *f.gauge.snapshot());
    assert_eq!(
        refusal.result.scope(),
        PlayResultScope::PracticeSection {
            start: Timestamp::ZERO,
            end: Some(Timestamp::from_nanos(10_000_000))
        }
    );
    assert_eq!(f.observer.as_ref().unwrap().marks, 1);
    assert_eq!(f.device.step, 3);
    assert!(
        f.device
            .report
            .unwrap()
            .playback_end_physical_frame
            .is_some()
    );
}

#[test]
fn solo_cancel_close_cutoff_or_technical_failure_never_manufactures_a_result() {
    struct Cutoff(VecDeque<u64>);
    impl NativePumpControl for Cutoff {
        type Moment = u64;
        fn now(&mut self) -> NativeGameplayResult<u64> {
            Ok(self.0.pop_front().ok_or("unexpected cutoff read")?)
        }
        fn checked_add(moment: u64, duration: std::time::Duration) -> Option<u64> {
            moment.checked_add(u64::try_from(duration.as_nanos()).ok()?)
        }
        fn wait(&mut self, _: std::time::Duration) -> NativeGameplayResult<()> {
            Ok(())
        }
    }
    for case in 0..5 {
        let finite = case == 4;
        let mut f = Fixture::new(finite, false);
        let mut host = ResultHost::default();
        let mut config = config(finite);
        match case {
            0 => host.base.cancel = true,
            1 => f.device.close = true,
            2 => config.seconds = Some(1),
            3 => f.device.reject_at = Some(1),
            _ => host.cancel_at = Some(Timestamp::from_nanos(10_000_000)),
        }
        let result = if case == 2 {
            f.run(config, &mut host, &mut Cutoff([0, 0, 1_000_000_000].into()))
        } else {
            f.run(config, &mut host, &mut Control::default())
        };
        if case == 3 {
            assert_eq!(
                result.unwrap_err().downcast_ref::<EstimatorError>(),
                Some(&EstimatorError::NonIncreasing)
            );
        } else {
            assert_eq!(result.unwrap(), None);
        }
        assert!(host.results.is_empty());
        assert_eq!(f.observer.as_ref().unwrap().marks, 0);
    }
}
