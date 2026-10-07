//! Deferred whole-cohort typed results through the actual shared output owner.
use crate::gameplay_presentation_port_fixtures::*;
use crate::{
    competition::ScoreSummary,
    local_input::InputMerger,
    local_players::PlayerId,
    local_runtime::{MemberConfig, RuntimeGroup},
    native_cohort::{CohortSession, GameplayPlayerState, run_cohort_with_results_and_ports},
    native_solo_result_fixtures::{
        PublicationRefused, ResultHost, ResultObserver, ResultTrace, completion, result_judge,
        result_source,
    },
    play_result::{
        CompletedLocalPublicationError, CompletedPlayResult, PlayResultOutcome, PlayResultScope,
    },
};
struct Fixture {
    device: MemoryDevice,
    group: RuntimeGroup,
    states: Vec<GameplayPlayerState<ResultObserver>>,
    merger: InputMerger,
    bgm: BgmFeeder,
    presentation: PresentationEstimator,
    pause: NativePause,
    end: Option<NativeEnd>,
    network: ResultGroup,
    delivery: InputDeliveryTelemetry,
    pre: u64,
}
#[derive(Default)]
struct ResultGroup {
    marks: usize,
    trace: ResultTrace,
}
impl GroupCompetitionPort for ResultGroup {
    fn observe(
        &mut self,
        _: &[crate::multiplayer_group::MemberProgress],
    ) -> NativeGameplayResult<()> {
        Ok(())
    }
    fn mark_native_completed(&mut self) {
        self.marks += 1;
        self.trace.borrow_mut().push("group-mark");
    }
}
impl Fixture {
    fn new(finite: bool, fatal: bool, both: bool) -> Self {
        let source = result_source(fatal);
        let sources = if fatal && !both {
            vec![DeviceId(1)]
        } else {
            vec![DeviceId(1), DeviceId(u64::MAX)]
        };
        let (mut device, producer) = device(finite, sources);
        device.close = false;
        let mut members = Vec::new();
        let mut states = Vec::new();
        let trace = ResultTrace::default();
        for (device, player) in [
            (DeviceId(1), PlayerId(7)),
            (DeviceId(u64::MAX), PlayerId(u32::MAX)),
        ] {
            members.push(MemberConfig {
                player,
                device: Some(device),
                bindings: bindings(Some(device)),
                judge: result_judge(&source, fatal),
                sounds: vec![],
            });
            states.push(GameplayPlayerState {
                player,
                capture: None,
                competition: Some(ResultObserver {
                    trace: trace.clone(),
                    label: if player == PlayerId(7) {
                        "first-mark"
                    } else {
                        "second-mark"
                    },
                    ..Default::default()
                }),
                completion: (!finite).then(|| completion(&source)),
                score: ScoreSummary::default(),
                gauge: BmsGauge::default(),
                last_song: Timestamp::ZERO,
            });
        }
        let mut group = RuntimeGroup::new(
            ClockDomainId(1),
            ClockDomainId(2),
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
            producer,
            members,
            8,
            &[],
        )
        .unwrap();
        group.set_processing_clock(RuntimeProcessingClock::Disabled);
        if finite {
            group
                .set_song_end(Timestamp::from_nanos(10_000_000))
                .unwrap();
        }
        let (pause, end) = pause_end(finite);
        Self {
            device,
            group,
            states,
            merger: InputMerger::new(
                ClockDomainId(1),
                point(1, 0),
                vec![DeviceId(1), DeviceId(u64::MAX)],
                16,
            )
            .unwrap(),
            bgm: bgm(),
            presentation: estimator(),
            pause,
            end,
            network: ResultGroup {
                trace,
                ..Default::default()
            },
            delivery: InputDeliveryTelemetry::new(16, ClockDomainId(1)).unwrap(),
            pre: 0,
        }
    }
    fn run(
        &mut self,
        config: NativeGameplayConfig,
        host: &mut ResultHost,
    ) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
        self.run_with_control(config, host, &mut Control::default())
    }
    fn run_with_control<C: NativePumpControl>(
        &mut self,
        config: NativeGameplayConfig,
        host: &mut ResultHost,
        control: &mut C,
    ) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
        host.trace = self.network.trace.clone();
        run_cohort_with_results_and_ports(
            &mut self.device,
            CohortSession {
                group: &mut self.group,
                network: Some(&mut self.network),
                states: &mut self.states,
                merger: &mut self.merger,
                bgm: &mut self.bgm,
                discipline: &mut self.presentation,
                pause: &mut self.pause,
                end: &mut self.end,
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
fn cohort_actual_shared_completion_returns_each_members_gauge_and_original_order() {
    for (finite, fatal, both) in [
        (true, false, false),
        (false, false, false),
        (false, true, false),
        (false, true, true),
    ] {
        let mut f = Fixture::new(finite, fatal, both);
        let mut host = ResultHost::default();
        let results = f.run(config(finite), &mut host).unwrap().unwrap();
        assert_eq!(
            results.iter().map(|row| row.0).collect::<Vec<_>>(),
            [PlayerId(7), PlayerId(u32::MAX)]
        );
        for (index, (player, result)) in results.iter().enumerate() {
            assert_eq!(*player, f.states[index].player);
            assert_eq!(result.gauge(), *f.states[index].gauge.snapshot());
            assert_eq!(
                result.scope(),
                if finite {
                    PlayResultScope::PracticeSection {
                        start: Timestamp::ZERO,
                        end: Some(Timestamp::from_nanos(10_000_000)),
                    }
                } else {
                    PlayResultScope::FullSong
                }
            );
            assert_eq!(
                result.outcome(),
                if fatal && (index == 0 || both) {
                    PlayResultOutcome::Failed(crate::gauge::GaugeFailure::InstantDeath)
                } else {
                    PlayResultOutcome::BelowClearThreshold
                }
            );
            assert!(!result.whole_song_clear());
        }
        assert_eq!(host.tables, [results]);
        assert_eq!(f.network.marks, 1);
        assert_eq!(
            &*host.trace.borrow(),
            &["first-mark", "second-mark", "group-mark", "local-result"]
        );
        assert!(
            f.states
                .iter()
                .all(|state| state.competition.as_ref().unwrap().marks == 1)
        );
        assert!(f.device.step >= 3);
        let output = f.device.report.unwrap();
        assert_eq!((output.active_voices, output.pending_commands), (0, 0));
        if fatal {
            assert_eq!(
                f.group.player_gameplay_fence(PlayerId(7)),
                Some(Timestamp::from_nanos(20_000_000))
            );
        }
    }
}

#[test]
fn cohort_refused_completion_publication_keeps_whole_proven_table_and_original_error() {
    let mut f = Fixture::new(true, false, false);
    let mut host = ResultHost {
        reject: true,
        ..Default::default()
    };
    let error = f.run(config(true), &mut host).unwrap_err();
    let refusal = error
        .downcast_ref::<CompletedLocalPublicationError>()
        .unwrap();
    assert!(refusal.cause.downcast_ref::<PublicationRefused>().is_some());
    assert_eq!(host.tables, [refusal.results.clone()]);
    assert_eq!(
        refusal.results.iter().map(|r| r.0).collect::<Vec<_>>(),
        [PlayerId(7), PlayerId(u32::MAX)]
    );
    assert_eq!(f.network.marks, 1);
    assert_eq!(
        &*host.trace.borrow(),
        &["first-mark", "second-mark", "group-mark", "local-result"]
    );
    assert!(
        f.states
            .iter()
            .all(|state| state.competition.as_ref().unwrap().marks == 1)
    );
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
fn cohort_cancellation_closed_input_and_bad_evidence_publish_no_completion_table() {
    struct Cutoff(usize);
    impl NativePumpControl for Cutoff {
        type Moment = u64;
        fn now(&mut self) -> NativeGameplayResult<u64> {
            self.0 += 1;
            assert!(self.0 <= 3, "unexpected cutoff read");
            Ok(if self.0 < 3 { 0 } else { 1_000_000_000 })
        }
        fn checked_add(moment: u64, duration: std::time::Duration) -> Option<u64> {
            moment.checked_add(u64::try_from(duration.as_nanos()).ok()?)
        }
        fn wait(&mut self, _: std::time::Duration) -> NativeGameplayResult<()> {
            Ok(())
        }
    }
    for case in 0..5 {
        let finite = case == 3;
        let mut f = Fixture::new(finite, false, false);
        let mut host = ResultHost::default();
        match case {
            0 => host.base.cancel = true,
            1 => f.device.close = true,
            2 => f.device.reject_at = Some(1),
            3 => host.cancel_at = Some(Timestamp::from_nanos(10_000_000)),
            _ => {}
        }
        let result = if case == 4 {
            f.run_with_control(
                NativeGameplayConfig {
                    seconds: Some(1),
                    ..config(false)
                },
                &mut host,
                &mut Cutoff(0),
            )
        } else {
            f.run(config(finite), &mut host)
        };
        if case == 2 {
            assert_eq!(
                result.unwrap_err().downcast_ref::<EstimatorError>(),
                Some(&EstimatorError::NonIncreasing)
            );
        } else {
            assert_eq!(result.unwrap(), None);
        }
        assert!(host.tables.is_empty());
        assert_eq!(f.network.marks, 0);
        assert!(
            f.states
                .iter()
                .all(|state| state.competition.as_ref().unwrap().marks == 0)
        );
    }
}

#[test]
fn cohort_clear_is_member_specific_and_nonzero_unbounded_start_remains_practice() {
    let mut f = Fixture::new(false, false, false);
    f.states[0].gauge = crate::native_solo_result_fixtures::cleared_gauge();
    let table = f
        .run(config(false), &mut ResultHost::default())
        .unwrap()
        .unwrap();
    assert_eq!(table[0].1.outcome(), PlayResultOutcome::Cleared);
    assert!(table[0].1.whole_song_clear());
    assert_eq!(table[1].1.outcome(), PlayResultOutcome::BelowClearThreshold);
    assert!(!table[1].1.whole_song_clear());
    let mut f = Fixture::new(false, false, false);
    let start = Timestamp::from_nanos(1);
    *f.group.transport_mut() = Transport::new(Timestamp::ZERO, start, Rate::NORMAL);
    let table = f
        .run(
            NativeGameplayConfig {
                song_origin: start,
                ..config(false)
            },
            &mut ResultHost::default(),
        )
        .unwrap()
        .unwrap();
    for (_, result) in table {
        assert_eq!(
            result.scope(),
            PlayResultScope::PracticeSection { start, end: None }
        );
        assert!(!result.whole_song_clear());
    }
}
