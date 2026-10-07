//! Live selected classes come from actual Runtime reports, independently of capture.
use super::*;
use crate::{
    judgment_policy::BmsScoreSummary,
    native_gameplay_bridge::PlayerGameplayHost,
    native_gameplay_host::{NativeGameplayHost, NativeScoreHost, NoopGameplayHost},
    play_policy::{ClassifiedWindow, ResolvedPlayPolicy},
};
use beatkernel::{
    audio::command_queue,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeGrade, JudgeOutcome, JudgeWindow},
    runtime::{Runtime, RuntimeProcessingClock},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration},
    transport::{Rate, Transport},
};
use beatkernel_bms::{BmsGaugeKind, BmsInputMode, BmsJudgment};

fn source() -> BmsChart {
    beatkernel_bms::parse(
        "#BPM 60\n#TOTAL 320\n#WAV01 x.wav\n#00011:01010101\n#00111:01",
        Default::default(),
    )
    .unwrap()
}
fn windows() -> [ClassifiedWindow; 4] {
    [
        (u32::MAX, BmsJudgment::PGreat),
        (0, BmsJudgment::Great),
        (77, BmsJudgment::Good),
        (7, BmsJudgment::Bad),
    ]
    .map(|(grade, judgment)| {
        let limit = match judgment {
            BmsJudgment::PGreat => 1,
            BmsJudgment::Great => 2,
            BmsJudgment::Good => 3,
            _ => 4,
        };
        ClassifiedWindow {
            judgment,
            window: JudgeWindow {
                grade: JudgeGrade(grade),
                early: Duration::from_nanos(limit),
                late: Duration::from_nanos(limit),
            },
        }
    })
}
fn policy(kind: BmsGaugeKind) -> ResolvedPlayPolicy {
    ResolvedPlayPolicy::bms(&source(), kind, &windows(), 0).unwrap()
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
fn runtime(policy: &ResolvedPlayPolicy) -> Runtime {
    let source = source();
    let judge = crate::mine_plan::prepare_judge(
        &source,
        source.compile().unwrap().chart,
        policy.judge().clone(),
        BmsInputMode::ButtonOnly,
        1024,
    )
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
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    runtime
}
fn input(ns: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(u64::MAX), point(ns), sequence),
        control: PhysicalControlId::keyboard(7u16),
        state,
    })
}
fn current() -> PlayerSnapshot {
    SESSION.with(|s| s.borrow().as_ref().unwrap().snapshot.clone())
}
fn unchanged(before: &PlayerSnapshot, after: &PlayerSnapshot) {
    assert_eq!(before.status, after.status);
    assert_eq!(before.song_time, after.song_time);
    assert_eq!(before.score, after.score);
    assert_eq!(before.bms_score, after.bms_score);
    assert_eq!(before.gauge, after.gauge);
    assert_eq!(before.mine_damage, after.mine_damage);
    assert_eq!(before.pressed_lanes, after.pressed_lanes);
    assert_eq!(before.last_judge, after.last_judge);
    assert_eq!(before.recent_results, after.recent_results);
    assert_eq!(before.players.len(), after.players.len());
    for (a, b) in before.players.iter().zip(&after.players) {
        assert_eq!(a.player, b.player);
        assert_eq!(a.song_time, b.song_time);
        assert_eq!(a.score, b.score);
        assert_eq!(a.bms_score, b.bms_score);
        assert_eq!(a.gauge, b.gauge);
        assert_eq!(a.mine_damage, b.mine_damage);
        assert_eq!(a.pressed_lanes, b.pressed_lanes);
        assert_eq!(a.last_judge, b.last_judge);
        assert_eq!(a.recent_results, b.recent_results);
        assert_eq!(
            format!("{:?}", a.note_progress),
            format!("{:?}", b.note_progress)
        );
    }
}
#[test]
fn actual_runtime_classes_reach_live_solo_for_six_gauges_with_and_without_recording() {
    for kind in BmsGaugeKind::ALL {
        for recording in [false, true] {
            let source = source();
            let policy = policy(kind);
            let mut runtime = runtime(&policy);
            let mut capture = crate::native_judge::prepare_section_capture_for_policy(
                &source,
                runtime.judge(),
                &policy,
                ClockDomainId(17),
                Timestamp::ZERO,
                0,
                None,
                crate::native_judge::capture_limits(recording, 65536, 128).unwrap(),
            )
            .unwrap();
            assert_eq!(capture.is_some(), recording);
            let (publisher, viewer) = channel();
            with_publisher(publisher, || {
                publish_chart(&source, runtime.judge().chart()).unwrap();
                let mut score = ScoreSummary::default();
                let mut ui = PlayerGameplayHost;
                let mut host = NativeScoreHost::new(&mut ui, &mut score);
                host.prepare_play_policies(&[(PlayerId(1), &policy)])
                    .unwrap();
                assert_eq!(current().bms_score, Some(BmsScoreSummary::default()));
                let mut expected_gauge = BmsGauge::new(policy.gauge().try_copy().unwrap());
                for (index, entry) in windows().iter().enumerate() {
                    let ns = index as i64 * 1_000_000_000 + index as i64 + 1;
                    for (offset, state) in
                        [ButtonState::Down, ButtonState::Up].into_iter().enumerate()
                    {
                        let report = runtime
                            .process_input(
                                input(ns, (index * 2 + offset) as u64, state),
                                &Identity,
                                point(ns),
                            )
                            .unwrap();
                        if offset == 0 {
                            assert!(matches!(report.judge_events[0].outcome,
                        JudgeOutcome::Hit {grade,..} if grade==entry.window.grade));
                        }
                        expected_gauge
                            .observe(&report.judge_events, &report.hazard_events)
                            .unwrap();
                        if let Some(capture) = &mut capture {
                            capture.record_report(&report).unwrap();
                        }
                        host.publish_report(&report).unwrap();
                    }
                }
                let report = runtime
                    .advance_to(point(4_000_000_005), &Identity, point(4_000_000_005))
                    .unwrap();
                assert_eq!(report.judge_events.len(), 1);
                expected_gauge
                    .observe(&report.judge_events, &report.hazard_events)
                    .unwrap();
                if let Some(capture) = &mut capture {
                    capture.record_report(&report).unwrap();
                }
                host.publish_report(&report).unwrap();
                // The public mirror is synchronized at the real coalesced handoff.
                publish_pause(PauseState::Paused);
                publish_pause(PauseState::Running);
                let played = viewer.take_latest().unwrap();
                assert_eq!(
                    played.bms_score,
                    Some(BmsScoreSummary {
                        pgreat: 1,
                        great: 1,
                        good: 1,
                        bad: 1,
                        poor: 1,
                        ex_score: 3
                    })
                );
                assert_eq!(played.players[0].bms_score, played.bms_score);
                assert_eq!(played.gauge, expected_gauge);
                assert_eq!(played.players[0].gauge, expected_gauge);
                drop(host);
                assert_eq!(played.score, score);
                let empty = runtime
                    .advance_to(point(4_000_000_006), &Identity, point(4_000_000_006))
                    .unwrap();
                assert!(empty.judge_events.is_empty());
                ui.publish_report(&empty).unwrap();
                assert_eq!(current().bms_score, played.bms_score);
                publish_pause(PauseState::Paused);
                publish_pause(PauseState::Running);
                assert_eq!(viewer.take_latest().unwrap().bms_score, played.bms_score);
                Ok(())
            })
            .unwrap();
            if let Some(capture) = capture {
                let setup = crate::replay_playback::decode_section_setup(
                    &capture.into_file().header.options,
                )
                .unwrap();
                assert_eq!(setup.judgments.as_ref(), policy.judgments());
            }
        }
    }
}
#[test]
fn builtin_and_legacy_gauge_only_remain_unclassified_and_new_session_clears_classes() {
    let source = source();
    let compiled = source.compile().unwrap();
    for full in [false, true] {
        let builtin = ResolvedPlayPolicy::builtin(4, 4, 0).unwrap();
        let mut runtime = runtime(&builtin);
        let (publisher, viewer) = channel();
        with_publisher(publisher, || {
            publish_chart(&source, &compiled.chart).unwrap();
            if full {
                prepare_native_play_policies(&[(PlayerId(1), &builtin)]).unwrap();
            } else {
                prepare_native_policies(&[(PlayerId(1), builtin.gauge())]).unwrap();
            }
            let report = runtime
                .process_input(input(1, 0, ButtonState::Down), &Identity, point(1))
                .unwrap();
            publish_report(&report).unwrap();
            publish_pause(PauseState::Paused);
            publish_pause(PauseState::Running);
            let published = viewer.take_latest().unwrap();
            assert_eq!(published.score.hits, 1);
            assert_eq!(published.players[0].score.hits, 1);
            assert_eq!(published.bms_score, None);
            assert_eq!(published.players[0].bms_score, None);
            Ok(())
        })
        .unwrap();
    }
    let selected = policy(BmsGaugeKind::Groove);
    let (publisher, _) = channel();
    with_publisher(publisher, || {
        publish_chart(&source, &compiled.chart).unwrap();
        prepare_native_play_policies(&[(PlayerId(1), &selected)]).unwrap();
        Ok(())
    })
    .unwrap();
    let (publisher, _) = channel();
    with_publisher(publisher, || {
        publish_chart(&source, &compiled.chart).unwrap();
        assert_eq!(current().bms_score, None);
        assert_eq!(current().players[0].bms_score, None);
        Ok(())
    })
    .unwrap();
}
#[test]
fn full_policy_cold_roster_validation_is_atomic_retryable_and_one_shot() {
    let source = source();
    let compiled = source.compile().unwrap();
    let selected = policy(BmsGaugeKind::Hard);
    for count in [1usize, 2, 64] {
        let ids = (0..count)
            .map(|i| PlayerId(u32::MAX - i as u32))
            .collect::<Vec<_>>();
        let rows = ids.iter().map(|id| (*id, &selected)).collect::<Vec<_>>();
        let (publisher, _) = channel();
        with_publisher(publisher, || {
            assert!(prepare_native_play_policies(&rows).is_err());
            publish_local_chart(&source, &compiled.chart, &ids).unwrap();
            let before = current();
            let mut invalid = rows.clone();
            invalid[0].0 = PlayerId(0);
            assert!(prepare_native_play_policies(&invalid).is_err());
            unchanged(&before, &current());
            assert!(prepare_native_play_policies(&[]).is_err());
            unchanged(&before, &current());
            if count > 1 {
                invalid = rows.clone();
                invalid.swap(0, 1);
                assert!(prepare_native_play_policies(&invalid).is_err());
                unchanged(&before, &current());
                invalid = rows.clone();
                invalid[1].0 = invalid[0].0;
                assert!(prepare_native_play_policies(&invalid).is_err());
                unchanged(&before, &current());
                assert!(prepare_native_play_policies(&rows[..count - 1]).is_err());
                unchanged(&before, &current());
            }
            prepare_native_play_policies(&rows).unwrap();
            let prepared = current();
            for (member, id) in prepared.players.iter().zip(ids.iter()) {
                assert_eq!(member.player, *id);
                assert_eq!(member.bms_score, Some(BmsScoreSummary::default()));
                assert_eq!(member.gauge.profile(), selected.gauge());
            }
            assert!(prepare_native_play_policies(&rows).is_err());
            unchanged(&prepared, &current());
            assert!(prepare_native_policies(&[(ids[0], selected.gauge())]).is_err());
            unchanged(&prepared, &current());
            Ok(())
        })
        .unwrap();
    }
    assert!(
        NoopGameplayHost
            .prepare_play_policies(&[(PlayerId(0), &selected)])
            .is_err()
    );
    assert!(
        NoopGameplayHost
            .prepare_play_policies(&[(PlayerId(7), &selected)])
            .is_ok()
    );
    assert!(
        NoopGameplayHost
            .prepare_play_policies(&vec![(PlayerId(7), &selected); 65])
            .is_err()
    );
    let (publisher, _) = channel();
    with_publisher(publisher, || {
        publish_chart(&source, &compiled.chart).unwrap();
        publish_replay_prefix(Timestamp::ZERO, &[]).unwrap();
        let before = current();
        assert!(prepare_native_play_policies(&[(PlayerId(1), &selected)]).is_err());
        unchanged(&before, &current());
        Ok(())
    })
    .unwrap();
}
#[test]
fn sparse_original_cohort_ids_keep_independent_real_class_scores_without_aggregate() {
    let source = source();
    let policies = [policy(BmsGaugeKind::Groove), policy(BmsGaugeKind::Hard)];
    let ids = [PlayerId(7), PlayerId(u32::MAX)];
    let mut runtimes = policies.iter().map(runtime).collect::<Vec<_>>();
    let (publisher, _) = channel();
    with_publisher(publisher, || {
        publish_local_chart(&source, &source.compile().unwrap().chart, &ids).unwrap();
        prepare_native_play_policies(&[(ids[0], &policies[0]), (ids[1], &policies[1])]).unwrap();
        let rows = runtimes
            .iter_mut()
            .enumerate()
            .map(|(i, r)| PlayerReport {
                player: ids[i],
                report: r
                    .process_input(
                        input(i as i64 + 1, 0, ButtonState::Down),
                        &Identity,
                        point(i as i64 + 1),
                    )
                    .unwrap(),
            })
            .collect::<Vec<_>>();
        publish_local_reports(&rows).unwrap();
        let played = current();
        assert_eq!(played.bms_score, None);
        assert_eq!(played.players[0].player, ids[0]);
        assert_eq!(played.players[1].player, ids[1]);
        assert_eq!(
            played.players[0].bms_score,
            Some(BmsScoreSummary {
                pgreat: 1,
                ex_score: 2,
                ..Default::default()
            })
        );
        assert_eq!(
            played.players[1].bms_score,
            Some(BmsScoreSummary {
                great: 1,
                ex_score: 1,
                ..Default::default()
            })
        );
        let rows = runtimes
            .iter_mut()
            .enumerate()
            .map(|(i, r)| PlayerReport {
                player: ids[i],
                report: r.advance_to(point(10), &Identity, point(10)).unwrap(),
            })
            .collect::<Vec<_>>();
        assert!(rows.iter().all(|r| r.report.judge_events.is_empty()));
        publish_local_reports(&rows).unwrap();
        for (a, b) in played.players.iter().zip(current().players.iter()) {
            assert_eq!(a.bms_score, b.bms_score);
            assert_eq!(a.gauge, b.gauge);
            assert_eq!(a.score, b.score);
        }
        Ok(())
    })
    .unwrap();
}
#[test]
fn unknown_later_member_class_refuses_every_publication_and_private_pressed_mutation() {
    let source = source();
    let selected = policy(BmsGaugeKind::Groove);
    let ids = [PlayerId(7), PlayerId(u32::MAX)];
    let mut runtimes = [runtime(&selected), runtime(&selected)];
    let (publisher, viewer) = channel();
    with_publisher(publisher, || {
        publish_local_chart(&source, &source.compile().unwrap().chart, &ids).unwrap();
        prepare_native_play_policies(&[(ids[0], &selected), (ids[1], &selected)]).unwrap();
        publish_pause(PauseState::Running);
        let publication = viewer.take_latest().unwrap();
        let before = current();
        let mut rows = runtimes
            .iter_mut()
            .enumerate()
            .map(|(i, r)| PlayerReport {
                player: ids[i],
                report: r
                    .process_input(input(1, 0, ButtonState::Down), &Identity, point(1))
                    .unwrap(),
            })
            .collect::<Vec<_>>();
        let JudgeOutcome::Hit { delta, .. } = rows[1].report.judge_events[0].outcome else {
            panic!("expected hit")
        };
        rows[1].report.judge_events[0].outcome = JudgeOutcome::Hit {
            grade: JudgeGrade(12345),
            delta,
        };
        assert!(publish_local_reports(&rows).is_err());
        unchanged(&before, &current());
        assert!(viewer.take_latest().is_none());
        unchanged(&before, &publication);
        SESSION.with(|s| {
            assert!(
                s.borrow()
                    .as_ref()
                    .unwrap()
                    .pressed
                    .iter()
                    .all(|p| p.keys.mask() == 0)
            )
        });
        Ok(())
    })
    .unwrap();
}
#[test]
fn ex_projection_overflow_in_later_member_preserves_entire_batch() {
    let source = source();
    let selected = policy(BmsGaugeKind::Groove);
    let ids = [PlayerId(7), PlayerId(u32::MAX)];
    let mut runtimes = [runtime(&selected), runtime(&selected)];
    let (publisher, _) = channel();
    with_publisher(publisher, || {
        publish_local_chart(&source, &source.compile().unwrap().chart, &ids).unwrap();
        prepare_native_play_policies(&[(ids[0], &selected), (ids[1], &selected)]).unwrap();
        // Keep generic accumulation valid; only 2*PG exceeds the class projection range.
        SESSION.with(|s| {
            let mut s = s.borrow_mut();
            let member = &mut s.as_mut().unwrap().snapshot.players[1];
            member.score.hits = u64::MAX / 2;
            member.score.grades.insert(u32::MAX, u64::MAX / 2);
            member.bms_score = Some(BmsScoreSummary {
                pgreat: u64::MAX / 2,
                ex_score: u64::MAX - 1,
                ..Default::default()
            });
        });
        let before = current();
        let rows = runtimes
            .iter_mut()
            .enumerate()
            .map(|(i, r)| PlayerReport {
                player: ids[i],
                report: r
                    .process_input(input(1, 0, ButtonState::Down), &Identity, point(1))
                    .unwrap(),
            })
            .collect::<Vec<_>>();
        assert!(publish_local_reports(&rows).is_err());
        unchanged(&before, &current());
        SESSION.with(|s| {
            assert!(
                s.borrow()
                    .as_ref()
                    .unwrap()
                    .pressed
                    .iter()
                    .all(|p| p.keys.mask() == 0)
            )
        });
        Ok(())
    })
    .unwrap();
}

struct HeaderCompetition(beatkernel::replay::ReplayHeader);
impl crate::gameplay_competition::SoloCompetitionPort for HeaderCompetition {
    fn expected_policy_header(&self) -> Option<&beatkernel::replay::ReplayHeader> {
        Some(&self.0)
    }
    fn observe(
        &mut self,
        _: &beatkernel::runtime::RuntimeReport,
    ) -> crate::native_gameplay::NativeGameplayResult<()> {
        Ok(())
    }
    fn mark_native_completed(&mut self) {}
}
#[test]
fn policy_aware_shared_bridge_refuses_mismatches_before_device_or_control_effects() {
    use crate::gameplay_presentation_port_fixtures as port;
    use crate::local_runtime::SoloRuntime;
    use crate::native_gameplay::{
        GameplaySession, run_gameplay_with_policy_and_result_and_score_and_ports,
    };
    for mismatch in [
        "judge",
        "gauge",
        "processed",
        "fenced",
        "poisoned",
        "capture-class",
        "competition-class",
    ] {
        let source = port::source();
        let selected = ResolvedPlayPolicy::bms(
            &source,
            BmsGaugeKind::Groove,
            &[ClassifiedWindow {
                judgment: BmsJudgment::PGreat,
                window: JudgeWindow {
                    grade: JudgeGrade(1),
                    early: Duration::ZERO,
                    late: Duration::ZERO,
                },
            }],
            0,
        )
        .unwrap();
        let other = ResolvedPlayPolicy::bms(
            &source,
            BmsGaugeKind::Groove,
            &[ClassifiedWindow {
                judgment: BmsJudgment::Great,
                window: selected.judge().windows()[0],
            }],
            0,
        )
        .unwrap();
        assert_eq!(selected.gauge(), other.gauge());
        assert_eq!(selected.judge(), other.judge());
        assert_ne!(selected.judgments(), other.judgments());
        let engine = crate::mine_plan::prepare_judge(
            &source,
            source.compile().unwrap().chart,
            if mismatch == "judge" {
                ResolvedPlayPolicy::builtin(1, 1, 0)
                    .unwrap()
                    .judge()
                    .clone()
            } else {
                selected.judge().clone()
            },
            BmsInputMode::ButtonOnly,
            1024,
        )
        .unwrap();
        let limits = crate::native_judge::capture_limits(true, 65536, 128)
            .unwrap()
            .unwrap();
        let different = crate::replay_capture::LiveReplayCapture::new_with_policy(
            &port::judge(&source),
            ClockDomainId(1),
            limits,
            Timestamp::ZERO,
            0,
            None,
            BmsInputMode::ButtonOnly,
            None,
            &other,
        )
        .unwrap();
        let mut capture = (mismatch == "capture-class").then_some(different);
        let mut competition = if mismatch == "competition-class" {
            Some(HeaderCompetition(
                crate::replay_capture::LiveReplayCapture::new_with_policy(
                    &port::judge(&source),
                    ClockDomainId(1),
                    limits,
                    Timestamp::ZERO,
                    0,
                    None,
                    BmsInputMode::ButtonOnly,
                    None,
                    &other,
                )
                .unwrap()
                .header()
                .clone(),
            ))
        } else {
            None
        };
        let (mut device, producer) = port::device(false, vec![]);
        let mut runtime = SoloRuntime::new(
            ClockDomainId(1),
            ClockDomainId(2),
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
            port::bindings(None),
            engine,
            producer,
            vec![],
            0,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        if mismatch == "processed" || mismatch == "fenced" {
            // Use actual matching domains to produce the committed empty prefix.
            struct Domains;
            impl ClockMapper for Domains {
                fn map(&self, p: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
                    Some(p.timestamp)
                }
                fn quality(&self) -> ClockMappingQuality {
                    ClockMappingQuality::Exact
                }
            }
            runtime
                .advance_to(port::point(1, 0), &Domains, port::point(2, 0))
                .unwrap();
        }
        if mismatch == "fenced" {
            assert_eq!(runtime.fence_gameplay(), Some(Timestamp::ZERO));
            assert_eq!(runtime.gameplay_fence(), Some(Timestamp::ZERO));
        }
        if mismatch == "poisoned" {
            struct PanicMapper;
            impl ClockMapper for PanicMapper {
                fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
                    panic!("cold mapper panic fixture")
                }
                fn quality(&self) -> ClockMappingQuality {
                    ClockMappingQuality::Exact
                }
            }
            let mut event = input(0, 0, ButtonState::Down);
            event.meta_mut().clock_domain = ClockDomainId(99);
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    runtime.process_input(event, &PanicMapper, port::point(2, 0))
                }))
                .is_err()
            );
            assert!(runtime.poisoned());
            assert_eq!(runtime.gameplay_fence(), None);
            assert_eq!(runtime.judge().effective_song_time(), None);
        }
        let mut gauge = if mismatch == "gauge" {
            BmsGauge::default()
        } else {
            BmsGauge::new(selected.gauge().try_copy().unwrap())
        };
        let before = gauge.clone();
        let mut bgm = port::bgm();
        let mut presentation = port::estimator();
        let (mut pause, mut end) = port::pause_end(false);
        let mut completion = None;
        let mut delivery =
            beatkernel::telemetry::InputDeliveryTelemetry::new(16, ClockDomainId(1)).unwrap();
        let mut pre = 0;
        let mut control = port::Control::default();
        let mut host = NoopGameplayHost;
        let mut score = ScoreSummary::default();
        assert!(
            run_gameplay_with_policy_and_result_and_score_and_ports(
                &mut device,
                GameplaySession {
                    runtime: &mut runtime,
                    gauge: &mut gauge,
                    bgm: &mut bgm,
                    discipline: &mut presentation,
                    pause: &mut pause,
                    end: &mut end,
                    completion: &mut completion,
                    capture: &mut capture,
                    competition: &mut competition,
                    delivery: &mut delivery,
                    pre_origin_inputs: &mut pre
                },
                port::config(false),
                &mut control,
                &mut host,
                &mut score,
                &selected
            )
            .is_err(),
            "{mismatch}"
        );
        assert_eq!(device.step, 0, "{mismatch}");
        assert!(device.pcm.is_empty());
        assert_eq!(control.waits, 0);
        assert_eq!(gauge, before);
        assert_eq!(score, ScoreSummary::default());
        assert_eq!(pre, 0);
    }
}

#[test]
fn solo_unknown_grade_and_class_only_overflow_preserve_snapshot_and_publication() {
    for overflow in [false, true] {
        let source = source();
        let selected = policy(BmsGaugeKind::Groove);
        let mut runtime = runtime(&selected);
        let (publisher, viewer) = channel();
        with_publisher(publisher, || {
            publish_chart(&source, runtime.judge().chart()).unwrap();
            prepare_native_play_policies(&[(PlayerId(1), &selected)]).unwrap();
            if overflow {
                SESSION.with(|s| {
                    let mut s = s.borrow_mut();
                    let snapshot = &mut s.as_mut().unwrap().snapshot;
                    snapshot.players[0].score.hits = u64::MAX / 2;
                    snapshot.players[0]
                        .score
                        .grades
                        .insert(u32::MAX, u64::MAX / 2);
                    snapshot.players[0].bms_score = Some(BmsScoreSummary {
                        pgreat: u64::MAX / 2,
                        ex_score: u64::MAX - 1,
                        ..Default::default()
                    });
                    snapshot.score = snapshot.players[0].score.clone();
                    snapshot.bms_score = snapshot.players[0].bms_score;
                });
            }
            publish_pause(PauseState::Running);
            viewer.take_latest().unwrap();
            let before = current();
            let mut report = runtime
                .process_input(input(1, 0, ButtonState::Down), &Identity, point(1))
                .unwrap();
            if !overflow {
                let JudgeOutcome::Hit { delta, .. } = report.judge_events[0].outcome else {
                    panic!("expected hit")
                };
                report.judge_events[0].outcome = JudgeOutcome::Hit {
                    grade: JudgeGrade(12345),
                    delta,
                };
            }
            assert!(publish_report(&report).is_err());
            unchanged(&before, &current());
            assert!(viewer.take_latest().is_none());
            SESSION.with(|s| assert_eq!(s.borrow().as_ref().unwrap().pressed[0].keys.mask(), 0));
            Ok(())
        })
        .unwrap();
    }
}

#[test]
fn selected_cohort_bridge_rejects_late_class_identity_and_roster_errors_without_effects() {
    use crate::{
        gameplay_presentation_port_fixtures as port,
        local_input::InputMerger,
        local_runtime::{MemberConfig, RuntimeGroup},
        native_cohort::{
            CohortSession, GameplayPlayerState, run_cohort_with_policies_and_results_and_ports,
        },
    };
    for mismatch in [
        "judge",
        "gauge",
        "capture-class",
        "competition-class",
        "order",
        "fenced",
    ] {
        let source = port::source();
        let ids = [PlayerId(7), PlayerId(u32::MAX)];
        let devices = [DeviceId(1), DeviceId(u64::MAX)];
        let selected = ResolvedPlayPolicy::bms(
            &source,
            BmsGaugeKind::Groove,
            &[ClassifiedWindow {
                judgment: BmsJudgment::PGreat,
                window: port::judge(&source).profile().windows()[0],
            }],
            0,
        )
        .unwrap();
        let other = ResolvedPlayPolicy::bms(
            &source,
            BmsGaugeKind::Groove,
            &[ClassifiedWindow {
                judgment: BmsJudgment::Great,
                window: selected.judge().windows()[0],
            }],
            0,
        )
        .unwrap();
        assert_eq!(selected.gauge(), other.gauge());
        assert_eq!(selected.judge(), other.judge());
        let (mut device, producer) = port::device(false, devices.to_vec());
        let members = ids
            .iter()
            .enumerate()
            .map(|(i, id)| MemberConfig {
                player: *id,
                device: Some(devices[i]),
                bindings: port::bindings(Some(devices[i])),
                judge: crate::mine_plan::prepare_judge(
                    &source,
                    source.compile().unwrap().chart,
                    if i == 1 && mismatch == "judge" {
                        ResolvedPlayPolicy::builtin(1, 1, 0)
                            .unwrap()
                            .judge()
                            .clone()
                    } else {
                        selected.judge().clone()
                    },
                    BmsInputMode::ButtonOnly,
                    1024,
                )
                .unwrap(),
                sounds: vec![],
            })
            .collect();
        let mut group = RuntimeGroup::new(
            ClockDomainId(1),
            ClockDomainId(2),
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
            producer,
            members,
            0,
            &[],
        )
        .unwrap();
        group.set_processing_clock(RuntimeProcessingClock::Disabled);
        let mut states: Vec<GameplayPlayerState<HeaderCompetition>> = ids
            .iter()
            .map(|id| GameplayPlayerState {
                player: *id,
                capture: None,
                competition: None,
                completion: None,
                score: ScoreSummary::default(),
                gauge: BmsGauge::new(selected.gauge().try_copy().unwrap()),
                last_song: Timestamp::ZERO,
            })
            .collect();
        if mismatch == "gauge" {
            states[1].gauge = BmsGauge::default();
        }
        if mismatch == "capture-class" || mismatch == "competition-class" {
            let capture = crate::replay_capture::LiveReplayCapture::new_with_policy(
                &port::judge(&source),
                ClockDomainId(1),
                crate::native_judge::capture_limits(true, 65536, 128)
                    .unwrap()
                    .unwrap(),
                Timestamp::ZERO,
                0,
                None,
                BmsInputMode::ButtonOnly,
                None,
                &other,
            )
            .unwrap();
            if mismatch == "capture-class" {
                states[1].capture = Some(capture);
            } else {
                states[1].competition = Some(HeaderCompetition(capture.header().clone()));
            }
        }
        if mismatch == "fenced" {
            struct Domains;
            impl ClockMapper for Domains {
                fn map(&self, p: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
                    Some(p.timestamp)
                }
                fn quality(&self) -> ClockMappingQuality {
                    ClockMappingQuality::Exact
                }
            }
            let prefix = group
                .advance_to(port::point(1, 0), &Domains, port::point(2, 0))
                .unwrap();
            assert!(prefix.iter().all(|row| row.report.judge_events.is_empty()));
            assert_eq!(group.fence_player(ids[1]).unwrap(), Some(Timestamp::ZERO));
            assert_eq!(group.player_gameplay_fence(ids[1]), Some(Timestamp::ZERO));
        }
        let before = states.iter().map(|s| s.gauge.clone()).collect::<Vec<_>>();
        let before_times = ids
            .iter()
            .map(|id| group.member_judge(*id).unwrap().effective_song_time())
            .collect::<Vec<_>>();
        let mut rows = [(ids[0], &selected), (ids[1], &selected)];
        if mismatch == "order" {
            rows.swap(0, 1);
        }
        let mut merger =
            InputMerger::new(ClockDomainId(1), port::point(1, 0), devices.to_vec(), 16).unwrap();
        let mut bgm = port::bgm();
        let mut presentation = port::estimator();
        let (mut pause, mut end) = port::pause_end(false);
        let mut delivery =
            beatkernel::telemetry::InputDeliveryTelemetry::new(16, ClockDomainId(1)).unwrap();
        let mut pre = 0;
        let mut control = port::Control::default();
        let mut host = NoopGameplayHost;
        let network: Option<&mut crate::gameplay_competition::NoopGroupCompetition> = None;
        assert!(
            run_cohort_with_policies_and_results_and_ports(
                &mut device,
                CohortSession {
                    group: &mut group,
                    network,
                    states: &mut states,
                    merger: &mut merger,
                    bgm: &mut bgm,
                    discipline: &mut presentation,
                    pause: &mut pause,
                    end: &mut end,
                    delivery: &mut delivery,
                    pre_origin_inputs: &mut pre
                },
                port::config(false),
                &mut control,
                &mut host,
                &rows
            )
            .is_err(),
            "{mismatch}"
        );
        assert_eq!(device.step, 0, "{mismatch}");
        assert!(device.pcm.is_empty());
        assert_eq!(control.waits, 0);
        for ((state, gauge), before_time) in states.iter().zip(before).zip(before_times) {
            assert_eq!(state.gauge, gauge);
            assert_eq!(state.score, ScoreSummary::default());
            assert_eq!(state.last_song, Timestamp::ZERO);
            assert_eq!(
                group
                    .member_judge(state.player)
                    .unwrap()
                    .effective_song_time(),
                before_time
            );
            if let Some(capture) = &state.capture {
                assert!(capture.records().is_empty());
            }
        }
        assert_eq!(pre, 0);
    }
}
#[test]
fn static_selected_host_matches_exact_ids_and_gauges_and_legacy_host_refuses_classes() {
    use crate::native_gameplay_bridge::ResolvedGameplayHost;
    let selected = policy(BmsGaugeKind::Groove);
    let source = source();
    let ids = [PlayerId(7), PlayerId(u32::MAX)];
    let rows = [(ids[0], &selected), (ids[1], &selected)];
    let (publisher, _) = channel();
    with_publisher(publisher, || {
        publish_local_chart(&source, &source.compile().unwrap().chart, &ids).unwrap();
        let before = current();
        let mut ui = PlayerGameplayHost;
        let mut adapter = ResolvedGameplayHost {
            host: &mut ui,
            policies: &rows,
        };
        assert!(
            adapter
                .prepare_policies(&[(ids[1], selected.gauge()), (ids[0], selected.gauge())])
                .is_err()
        );
        unchanged(&before, &current());
        assert!(
            adapter
                .prepare_policies(&[
                    (ids[0], &GaugeProfile::default()),
                    (ids[1], selected.gauge())
                ])
                .is_err()
        );
        unchanged(&before, &current());
        adapter
            .prepare_policies(&[(ids[0], selected.gauge()), (ids[1], selected.gauge())])
            .unwrap();
        assert!(
            current()
                .players
                .iter()
                .all(|member| member.bms_score.is_some())
        );
        Ok(())
    })
    .unwrap();
    let mut legacy = crate::gameplay_presentation_port_fixtures::Host::default();
    assert!(
        legacy
            .prepare_play_policies(&[(PlayerId(1), &selected)])
            .is_err()
    );
    assert!(legacy.solo.is_empty());
    assert!(legacy.local.is_empty());
}
