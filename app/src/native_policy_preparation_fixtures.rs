use crate::{
    gauge::{BmsGauge, GaugeProfile, GaugeFailure},
    local_players::PlayerId,
    native_gameplay_host::{NativeGameplayHost, NativeScoreHost, NoopGameplayHost},
    native_gameplay_bridge::PlayerGameplayHost,
    play_policy::{ResolvedPlayPolicy, ClassifiedWindow},
    player::{self, PauseState, PlayerViewer, PlayerSnapshot},
};
use beatkernel::{
    audio::command_queue,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent, codec::CodecLimits,
    },
    judge::{JudgeGrade, JudgeWindow},
    replay::codec::ReplayCodecLimits,
    runtime::{Runtime, RuntimeProcessingClock},
    time::{ClockDomainId, ClockPoint, ClockMapper, ClockMappingQuality, Timestamp, Duration},
    transport::{Transport, Rate},
};
fn source() -> beatkernel_bms::BmsChart {
    beatkernel_bms::parse("#BPM 60\n#WAV01 note.wav\n#00011:01", Default::default()).unwrap()
}
fn policy(
    source: &beatkernel_bms::BmsChart,
    kind: beatkernel_bms::BmsGaugeKind,
) -> ResolvedPlayPolicy {
    ResolvedPlayPolicy::bms(
        source,
        kind,
        &[ClassifiedWindow {
            judgment: beatkernel_bms::BmsJudgment::Bad,
            window: JudgeWindow {
                grade: JudgeGrade(u32::MAX),
                early: Duration::ZERO,
                late: Duration::ZERO,
            },
        }],
        0,
    )
    .unwrap()
}
fn point(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(17),
        timestamp: Timestamp::from_nanos(ns),
    }
}
struct Identity;
impl ClockMapper for Identity {
    fn map(&self, p: ClockPoint, target: ClockDomainId) -> Option<Timestamp> {
        (p.domain == target).then_some(p.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn snapshot(viewer: &PlayerViewer) -> PlayerSnapshot {
    player::publish_pause(PauseState::Paused);
    player::publish_pause(PauseState::Running);
    viewer.take_latest().unwrap()
}
fn unchanged(a: &PlayerSnapshot, b: &PlayerSnapshot) {
    assert_eq!(a.status, b.status);
    assert_eq!(a.song_time, b.song_time);
    assert_eq!(a.score, b.score);
    assert_eq!(a.gauge, b.gauge);
    assert_eq!(a.pressed_lanes, b.pressed_lanes);
    assert_eq!(a.players.len(), b.players.len());
    for (a, b) in a.players.iter().zip(&b.players) {
        assert_eq!(a.player, b.player);
        assert_eq!(a.gauge, b.gauge);
        assert_eq!(a.song_time, b.song_time);
        assert_eq!(a.score, b.score);
        assert_eq!(a.mine_damage, b.mine_damage);
        assert_eq!(a.pressed_lanes, b.pressed_lanes);
    }
}
#[test]
fn actual_native_host_score_wrapper_capture_and_replay_preserve_all_six_policies() {
    for kind in beatkernel_bms::BmsGaugeKind::ALL {
        let source = source();
        let policy = policy(&source, kind);
        let engine = crate::mine_plan::prepare_judge(
            &source,
            source.compile().unwrap().chart,
            policy.judge().clone(),
            beatkernel_bms::BmsInputMode::ButtonOnly,
            1024,
        )
        .unwrap();
        let limits =
            ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap())
                .unwrap();
        let mut capture = crate::native_judge::prepare_section_capture_for_policy(
            &source,
            &engine,
            &policy,
            ClockDomainId(17),
            Timestamp::ZERO,
            0,
            None,
            Some(limits),
        )
        .unwrap()
        .unwrap();
        let bindings = BindingMap::from_bindings([Binding {
            device: DeviceSelector::Any,
            physical: PhysicalControlId::keyboard(7u16),
            game_control: GameControlId(0x11),
        }])
        .unwrap();
        let (producer, _consumer) = command_queue(8).unwrap();
        let mut runtime = Runtime::new(
            ClockDomainId(17),
            ClockDomainId(17),
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
            bindings,
            engine,
            producer,
            vec![],
            0,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        let (publisher, viewer) = player::channel();
        player::with_publisher(publisher, || {
            player::publish_chart(&source, runtime.judge().chart()).unwrap();
            let mut score = crate::competition::ScoreSummary::default();
            let mut ui = PlayerGameplayHost;
            {
                let mut host = NativeScoreHost::new(&mut ui, &mut score);
                host.prepare_policies(&[(PlayerId(1), policy.gauge())])
                    .unwrap();
                let initial = snapshot(&viewer);
                assert_eq!(initial.gauge.profile(), policy.gauge());
                assert_eq!(
                    initial.gauge.snapshot().level_units,
                    policy.gauge().initial_units()
                );
                let input = PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(DeviceId(9), point(0), 0),
                    control: PhysicalControlId::keyboard(7u16),
                    state: ButtonState::Down,
                });
                let report = runtime.process_input(input, &Identity, point(0)).unwrap();
                capture.record_report(&report).unwrap();
                host.publish_report(&report).unwrap();
            }
            let observed = snapshot(&viewer);
            assert_eq!(observed.score, score);
            assert_eq!(score.hits, 1);
            if matches!(
                kind,
                beatkernel_bms::BmsGaugeKind::Hazard | beatkernel_bms::BmsGaugeKind::ExHard
            ) {
                assert_eq!(
                    observed.gauge.snapshot().failure,
                    Some(GaugeFailure::Depleted)
                );
                assert_eq!(observed.pressed_lanes, 0);
            } else {
                assert_eq!(observed.gauge.snapshot().failure, None);
                assert_ne!(observed.pressed_lanes, 0);
            }
            let file = capture.into_file();
            let setup = crate::replay_playback::decode_section_setup(&file.header.options).unwrap();
            assert_eq!(&setup.gauge, policy.gauge());
            let mut replay =
                crate::replay_visual::ReplayVisual::new_section(&source, &file, limits).unwrap();
            replay.advance_to(Timestamp::ZERO).unwrap();
            assert_eq!(replay.gauge(), &observed.gauge);
            Ok(())
        })
        .unwrap();
    }
}
#[test]
fn whole_roster_initialization_preserves_full_width_ids_and_refuses_atomically_once() {
    let source = source();
    let compiled = source.compile().unwrap();
    let custom = policy(&source, beatkernel_bms::BmsGaugeKind::Hard);
    for count in [1usize, 2, 64] {
        let ids = (0..count)
            .map(|i| PlayerId(u32::MAX - i as u32))
            .collect::<Vec<_>>();
        let (publisher, viewer) = player::channel();
        player::with_publisher(publisher, || {
            player::publish_local_chart(&source, &compiled.chart, &ids).unwrap();
            let before = snapshot(&viewer);
            let rows = ids
                .iter()
                .map(|id| (*id, custom.gauge()))
                .collect::<Vec<_>>();
            let mut wrong = rows.clone();
            wrong[0].0 = PlayerId(0);
            assert!(player::prepare_native_policies(&wrong).is_err());
            unchanged(&before, &snapshot(&viewer));
            if count > 1 {
                let mut wrong = rows.clone();
                wrong.swap(0, 1);
                assert!(player::prepare_native_policies(&wrong).is_err());
                unchanged(&before, &snapshot(&viewer));
            }
            player::prepare_native_policies(&rows).unwrap();
            let prepared = snapshot(&viewer);
            for (row, id) in prepared.players.iter().zip(&ids) {
                assert_eq!(row.player, *id);
                assert_eq!(row.gauge.profile(), custom.gauge());
                assert_eq!(row.gauge.snapshot().level_units, 100_000_000);
            }
            assert!(player::prepare_native_policies(&rows).is_err());
            unchanged(&prepared, &snapshot(&viewer));
            Ok(())
        })
        .unwrap();
    }
    assert!(NoopGameplayHost.prepare_policies(&[]).is_err());
    assert!(
        NoopGameplayHost
            .prepare_policies(&[(PlayerId(0), custom.gauge())])
            .is_err()
    );
    assert!(
        NoopGameplayHost
            .prepare_policies(&[(PlayerId(7), custom.gauge())])
            .is_ok()
    );
    assert!(
        NoopGameplayHost
            .prepare_policies(&vec![(PlayerId(7), custom.gauge()); 65])
            .is_err()
    );
}
#[test]
fn native_capture_rejects_mismatched_or_processed_judge_and_disabled_source_stays_unused() {
    let mut source = beatkernel_bms::parse(
        "#BPM 60\n#WAV01 note.wav\n#00011:01\n#00031:01",
        Default::default(),
    )
    .unwrap();
    source.metadata.insert("VOLWAV".into(), "bad".into());
    let policy = policy(&source, beatkernel_bms::BmsGaugeKind::Hard);
    let mut judge = crate::mine_plan::prepare_judge(
        &source,
        source.compile().unwrap().chart,
        policy.judge().clone(),
        beatkernel_bms::BmsInputMode::ButtonOnly,
        1024,
    )
    .unwrap();
    assert!(
        crate::native_judge::prepare_section_capture_for_policy(
            &source,
            &judge,
            &policy,
            ClockDomainId(17),
            Timestamp::ZERO,
            0,
            None,
            None
        )
        .unwrap()
        .is_none()
    );
    let limits =
        ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap();
    assert!(
        crate::native_judge::prepare_section_capture_for_policy(
            &source,
            &judge,
            &policy,
            ClockDomainId(17),
            Timestamp::ZERO,
            0,
            None,
            Some(limits)
        )
        .is_err()
    );
    let builtin = ResolvedPlayPolicy::builtin(0, 0, 0).unwrap();
    assert!(
        crate::native_judge::prepare_section_capture_for_policy(
            &source,
            &judge,
            &builtin,
            ClockDomainId(17),
            Timestamp::ZERO,
            0,
            None,
            None
        )
        .is_err()
    );
    judge.advance_to(Timestamp::ZERO).unwrap();
    assert!(
        crate::native_judge::prepare_section_capture_for_policy(
            &source,
            &judge,
            &policy,
            ClockDomainId(17),
            Timestamp::ZERO,
            0,
            None,
            None
        )
        .is_err()
    );
}
#[test]
fn preparation_before_chart_or_after_empty_report_is_refused_without_rewriting_state() {
    let source = source();
    let compiled = source.compile().unwrap();
    let policy = policy(&source, beatkernel_bms::BmsGaugeKind::Hard);
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        assert!(player::prepare_native_policies(&[(PlayerId(1), policy.gauge())]).is_err());
        player::publish_chart(&source, &compiled.chart).unwrap();
        player::publish_replay_prefix(Timestamp::ZERO, &[]).unwrap();
        let before = snapshot(&viewer);
        assert!(player::prepare_native_policies(&[(PlayerId(1), policy.gauge())]).is_err());
        unchanged(&before, &snapshot(&viewer));
        Ok(())
    })
    .unwrap();
    assert_eq!(BmsGauge::default().profile(), &GaugeProfile::default());
}

#[test]
fn completed_solo_and_cohort_tables_close_policy_setup_even_without_song_progress() {
    let source = source();
    let compiled = source.compile().unwrap();
    let custom = policy(&source, beatkernel_bms::BmsGaugeKind::Hard);
    for ids in [vec![PlayerId(1)], vec![PlayerId(7), PlayerId(u32::MAX)]] {
        let (publisher, viewer) = player::channel();
        player::with_publisher(publisher, || {
            player::publish_local_chart(&source, &compiled.chart, &ids).unwrap();
            let result = crate::play_result::CompletedPlayResult::from_completed(
                Timestamp::ZERO,
                None,
                &BmsGauge::default(),
            );
            if ids.len() == 1 {
                player::publish_completed_solo(result).unwrap();
            } else {
                player::publish_completed_local(
                    &ids.iter().map(|id| (*id, result)).collect::<Vec<_>>(),
                )
                .unwrap();
            }
            let before = snapshot(&viewer);
            assert!(before.completed_results.is_some());
            assert!(before.players.iter().all(|row| row.song_time.is_none()));
            let rows = ids
                .iter()
                .map(|id| (*id, custom.gauge()))
                .collect::<Vec<_>>();
            assert!(player::prepare_native_policies(&rows).is_err());
            let after = snapshot(&viewer);
            unchanged(&before, &after);
            assert_eq!(before.completed_results, after.completed_results);
            Ok(())
        })
        .unwrap();
    }
}
#[test]
fn mixed_cohort_profiles_observe_real_reports_and_empty_deadlines_independently() {
    let source = source();
    let ids = [PlayerId(7), PlayerId(u32::MAX)];
    let policies = [
        policy(&source, beatkernel_bms::BmsGaugeKind::Hard),
        policy(&source, beatkernel_bms::BmsGaugeKind::Hazard),
    ];
    let mut runtimes = policies
        .iter()
        .map(|policy| {
            let judge = crate::mine_plan::prepare_judge(
                &source,
                source.compile().unwrap().chart,
                policy.judge().clone(),
                beatkernel_bms::BmsInputMode::ButtonOnly,
                1024,
            )
            .unwrap();
            let bindings = BindingMap::from_bindings([Binding {
                device: DeviceSelector::Any,
                physical: PhysicalControlId::keyboard(7u16),
                game_control: GameControlId(0x11),
            }])
            .unwrap();
            let (producer, consumer) = command_queue(8).unwrap();
            let mut runtime = Runtime::new(
                ClockDomainId(17),
                ClockDomainId(17),
                Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
                bindings,
                judge,
                producer,
                vec![],
                0,
            )
            .unwrap();
            runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
            (runtime, consumer)
        })
        .collect::<Vec<_>>();
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::publish_local_chart(&source, &source.compile().unwrap().chart, &ids).unwrap();
        let mut host = PlayerGameplayHost;
        host.prepare_policies(&[(ids[0], policies[0].gauge()), (ids[1], policies[1].gauge())])
            .unwrap();
        let reports = runtimes
            .iter_mut()
            .enumerate()
            .map(|(index, (runtime, _))| {
                let input = PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(DeviceId(100 + index as u64), point(0), 0),
                    control: PhysicalControlId::keyboard(7u16),
                    state: ButtonState::Down,
                });
                crate::local_runtime::PlayerReport {
                    player: ids[index],
                    report: runtime.process_input(input, &Identity, point(0)).unwrap(),
                }
            })
            .collect::<Vec<_>>();
        host.publish_local_reports(&reports).unwrap();
        let played = snapshot(&viewer);
        assert_eq!(played.players[0].gauge.profile(), policies[0].gauge());
        assert_eq!(played.players[1].gauge.profile(), policies[1].gauge());
        assert_eq!(played.players[0].gauge.snapshot().failure, None);
        assert_eq!(
            played.players[1].gauge.snapshot().failure,
            Some(GaugeFailure::Depleted)
        );
        let deadlines = runtimes
            .iter_mut()
            .enumerate()
            .map(|(index, (runtime, _))| crate::local_runtime::PlayerReport {
                player: ids[index],
                report: runtime.advance_to(point(1), &Identity, point(1)).unwrap(),
            })
            .collect::<Vec<_>>();
        assert!(
            deadlines
                .iter()
                .all(|row| row.report.judge_events.is_empty())
        );
        host.publish_local_reports(&deadlines).unwrap();
        let after = snapshot(&viewer);
        for (before, after) in played.players.iter().zip(&after.players) {
            assert_eq!(before.player, after.player);
            assert_eq!(before.gauge, after.gauge);
            assert_eq!(before.score, after.score);
        }
        Ok(())
    })
    .unwrap();
}

struct LegacyHost;
impl NativeGameplayHost for LegacyHost {
    fn cancelled(&self) -> bool {
        false
    }
    fn pause_requested(&self) -> bool {
        false
    }
    fn retry_pause_publication(&mut self) {}
    fn publish_pause(&mut self, _: PauseState) {}
    fn publish_section_end(&mut self, _: Timestamp) {}
    fn publish_report(
        &mut self,
        _: &beatkernel::runtime::RuntimeReport,
    ) -> crate::native_gameplay::NativeGameplayResult<()> {
        Ok(())
    }
    fn publish_local_reports(
        &mut self,
        _: &[crate::local_runtime::PlayerReport],
    ) -> crate::native_gameplay::NativeGameplayResult<()> {
        Ok(())
    }
    fn diagnostic(&mut self, _: crate::native_gameplay_host::NativeGameplayDiagnostic<'_>) {}
}
#[test]
fn legacy_host_requires_explicit_custom_capability_and_score_wrapper_does_not_bypass_it() {
    let source = source();
    let custom = policy(&source, beatkernel_bms::BmsGaugeKind::Hard);
    let mut legacy = LegacyHost;
    let mut score = crate::competition::ScoreSummary::default();
    {
        let mut wrapper = NativeScoreHost::new(&mut legacy, &mut score);
        wrapper
            .prepare_policies(&[(PlayerId(1), &GaugeProfile::default())])
            .unwrap();
        assert!(
            wrapper
                .prepare_policies(&[(PlayerId(1), custom.gauge())])
                .is_err()
        );
        assert!(wrapper.prepare_policies(&[]).is_err());
    }
    assert_eq!(score, crate::competition::ScoreSummary::default());
}
