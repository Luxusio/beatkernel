//! Semantic publication/transaction regressions. The allocator lives only in
//! the integration binary; this unsafe-free module uses actual owner behavior.
use super::*;
use crate::pressed_keys::fixtures::button;
use beatkernel::{
    input::ButtonState,
    judge::{HazardId, HazardOutcome},
};

mod fixtures {
    use super::*;
    pub(super) fn chart_fixture() -> (BmsChart, CompiledChart) {
        let source = beatkernel_bms::parse(
            "#TITLE Allocation\n#BPM 120\n#WAV01 tap.wav\n#00011:01\n",
            Default::default(),
        )
        .unwrap();
        let chart = source.compile().unwrap().chart;
        (source, chart)
    }
    pub(super) fn report(player: u32, nanos: i64, hits: usize, misses: usize) -> PlayerReport {
        use beatkernel::{
            chart::ObjectId,
            judge::{JudgeGrade, JudgeOutcome, JudgeStage, MissReason},
            time::{ClockDomainId, ClockMappingQuality, ClockPoint, Duration},
        };
        let song_time = Timestamp::from_nanos(nanos);
        PlayerReport {
            player: PlayerId(player),
            report: RuntimeReport {
                input: None,
                bound_inputs: Vec::new(),
                song_time,
                song_end_reached: false,
                audio_at: ClockPoint {
                    domain: ClockDomainId(2),
                    timestamp: song_time,
                },
                input_mapping_quality: ClockMappingQuality::Unknown,
                audio_mapping_quality: ClockMappingQuality::Unknown,
                judge_events: (0..hits + misses)
                    .map(|index| JudgeEvent {
                        object: ObjectId(index as u64 + 1),
                        stage: JudgeStage::Instant,
                        outcome: if index < hits {
                            JudgeOutcome::Hit {
                                grade: JudgeGrade(1),
                                delta: Duration::ZERO,
                            }
                        } else {
                            JudgeOutcome::Miss {
                                reason: MissReason::HeadTimeout,
                            }
                        },
                        at: song_time,
                        input: None,
                    })
                    .collect(),
                hazard_events: Vec::new(),
                judge_error: None,
                audio_commands: Vec::new(),
                audio_failures: Vec::new(),
            },
        }
    }
}

fn profile() -> GaugeProfile {
    use crate::gauge::GradeDelta;
    use beatkernel::judge::JudgeGrade;
    GaugeProfile::new(
        20_000_000,
        80_000_000,
        19,
        -1_000_000,
        true,
        vec![GradeDelta {
            grade: JudgeGrade(1),
            delta: 7,
        }],
    )
    .unwrap()
}
fn snapshot() -> PlayerSnapshot {
    SESSION.with(|owner| owner.borrow().as_ref().unwrap().snapshot.clone())
}
fn assert_members_equal(actual: &PlayerSnapshot, expected: &PlayerSnapshot) {
    assert_eq!(actual.players.len(), expected.players.len());
    for (actual, expected) in actual.players.iter().zip(&expected.players) {
        assert_eq!(actual.player, expected.player);
        assert_eq!(actual.song_time, expected.song_time);
        assert_eq!(actual.score, expected.score);
        assert_eq!(actual.bms_score, expected.bms_score);
        assert_eq!(actual.mine_damage, expected.mine_damage);
        assert_eq!(actual.gauge, expected.gauge);
        assert_eq!(actual.last_judge, expected.last_judge);
        assert_eq!(actual.recent_results, expected.recent_results);
        assert_eq!(actual.pressed_lanes, expected.pressed_lanes);
        match (&actual.chart, &expected.chart) {
            (Some(a), Some(b)) => assert!(Arc::ptr_eq(a, b)),
            (None, None) => (),
            _ => panic!("refusal changed registered chart"),
        }
    }
    assert_eq!(actual.status, expected.status);
    assert_eq!(actual.cancelled, expected.cancelled);
    assert_eq!(actual.pause, expected.pause);
}
fn hazard(value: u64) -> HazardEvent {
    HazardEvent {
        id: HazardId(1),
        at: Timestamp::ZERO,
        control: beatkernel::input::GameControlId(0x11),
        value,
        outcome: HazardOutcome::Triggered,
        input: None,
    }
}

#[test]
fn custom_policy_cohort_refusals_preserve_every_member_and_allow_valid_recovery() {
    let (source, chart) = fixtures::chart_fixture();
    let policy = profile();
    let (publisher, viewer) = channel();
    with_publisher(publisher, || {
        publish_local_chart(&source, &chart, &[PlayerId(1), PlayerId(2)]).unwrap();
        prepare_native_policies(&[(PlayerId(1), &policy), (PlayerId(2), &policy)]).unwrap();
        let mut seed = fixtures::report(2, 1, 1, 0);
        seed.report.bound_inputs = vec![button(7, 4, 0x11, ButtonState::Down)];
        publish_local_reports(&[seed]).unwrap();
        let before = snapshot();
        for bad_id in [0, 99] {
            assert!(publish_local_reports(&[
                fixtures::report(1, 2, 1, 0),
                fixtures::report(bad_id, 2, 1, 0)
            ])
            .is_err());
            assert_members_equal(&snapshot(), &before);
        }
        assert!(publish_local_reports(&[
            fixtures::report(1, 2, 1, 0),
            fixtures::report(1, 2, 1, 0)
        ])
        .is_err());
        assert_members_equal(&snapshot(), &before);
        for bad_damage in [0, 1296, u64::MAX] {
            let mut first = fixtures::report(1, 2, 1, 0);
            first.report.hazard_events = vec![hazard(1)];
            first.report.bound_inputs = vec![button(8, 5, 0x12, ButtonState::Down)];
            let mut second = fixtures::report(2, 2, 1, 0);
            second.report.hazard_events = vec![hazard(1), hazard(bad_damage)];
            assert!(publish_local_reports(&[first, second]).is_err());
            assert_members_equal(&snapshot(), &before);
        }
        SESSION.with(|owner| {
            owner.borrow_mut().as_mut().unwrap().snapshot.players[1]
                .score
                .hits = u64::MAX
        });
        let overflow = snapshot();
        let mut first = fixtures::report(1, 2, 1, 0);
        first.report.hazard_events = vec![hazard(1)];
        assert!(publish_local_reports(&[first, fixtures::report(2, 2, 1, 0)]).is_err());
        assert_members_equal(&snapshot(), &overflow);
        SESSION.with(|owner| {
            owner.borrow_mut().as_mut().unwrap().snapshot.players[1]
                .score
                .hits = before.players[1].score.hits
        });
        let mut excessive = fixtures::report(2, 2, 1, 0);
        excessive.report.bound_inputs = (100..4197)
            .map(|id| button(id, 4, 0x11, ButtonState::Down))
            .collect();
        let mut first = fixtures::report(1, 2, 1, 0);
        first.report.hazard_events = vec![hazard(1)];
        first.report.bound_inputs = vec![button(8, 5, 0x12, ButtonState::Down)];
        assert!(publish_local_reports(&[first, excessive]).is_err());
        assert_members_equal(&snapshot(), &before);
        // This release must still target the owner admitted before every refusal.
        let mut recovery = fixtures::report(2, 3, 1, 0);
        recovery.report.bound_inputs = vec![button(7, 4, 0x11, ButtonState::Up)];
        publish_local_reports(&[fixtures::report(1, 3, 1, 0), recovery]).unwrap();
        publish_pause(PauseState::Running);
        let recovered = viewer.take_latest().unwrap();
        assert_eq!(recovered.players[0].score.hits, 1);
        assert_eq!(recovered.players[1].score.hits, 2);
        assert!(recovered
            .players
            .iter()
            .all(|member| member.mine_damage == MineDamageSummary::default()
                && member.pressed_lanes == 0));
        assert_eq!(
            recovered.players[0].gauge.snapshot().level_units,
            policy.initial_units() + 7
        );
        assert_eq!(
            recovered.players[1].gauge.snapshot().level_units,
            policy.initial_units() + 14
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn contended_mailbox_retains_latest_ordered_history_and_owned_deliveries() {
    let policy = profile();
    let (source, chart) = fixtures::chart_fixture();
    let (publisher, viewer) = channel();
    with_publisher(publisher, || {
        publish_chart(&source, &chart).unwrap();
        prepare_native_policies(&[(PlayerId(1), &policy)]).unwrap();
        viewer.take_latest();
        let first = fixtures::report(1, 10, 129, 0).report;
        publish_report(&first).unwrap();
        publish_pause(PauseState::Running);
        let mut delivered = viewer.take_latest().unwrap();
        assert_eq!(delivered.players[0].recent_results, first.judge_events[1..]);
        assert_eq!(delivered.recent_results, first.judge_events[1..]);
        assert_ne!(
            delivered.recent_results.as_ptr(),
            delivered.players[0].recent_results.as_ptr()
        );
        let held = viewer.0.latest.lock().unwrap();
        let mut next = fixtures::report(1, 20, 1, 0).report;
        next.judge_events[0].object = beatkernel::chart::ObjectId(9000);
        publish_report(&next).unwrap();
        publish_pause(PauseState::Paused);
        publish_report(&fixtures::report(1, 30, 0, 0).report).unwrap();
        assert!(held.is_none());
        drop(held);
        retry_pause_publication();
        let current = viewer.take_latest().unwrap();
        let mut expected = first.judge_events[2..].to_vec();
        expected.extend_from_slice(&next.judge_events);
        assert_eq!(current.players[0].recent_results, expected);
        assert_eq!(current.recent_results, expected);
        assert_eq!(current.song_time, Some(Timestamp::from_nanos(30)));
        assert_eq!(current.score.hits, 130);
        assert_eq!(
            current.gauge.snapshot().level_units,
            policy.initial_units() + 130 * 7
        );
        delivered.players[0].recent_results.clear();
        delivered.recent_results[0].object = beatkernel::chart::ObjectId(u64::MAX);
        delivered.players[0].score.grades.clear();
        assert_eq!(current.players[0].recent_results, expected);
        assert_eq!(current.players[0].score.grades.get(&1), Some(&130));
        publish_pause(PauseState::Running);
        let again = viewer.take_latest().unwrap();
        assert_eq!(again.recent_results, expected);
        assert_eq!(again.players[0].recent_results, expected);
        retry_pause_publication();
        assert!(viewer.take_latest().is_none());
        Ok(())
    })
    .unwrap();
    let terminal = viewer.take_latest().unwrap();
    assert_eq!(terminal.status, PlayerStatus::Finished);
    assert_eq!(terminal.score.hits, 130);
}

#[test]
fn solo_replay_policy_refusal_failure_latch_and_pressed_recovery_remain_atomic() {
    let policy = profile();
    let (publisher, viewer) = channel();
    with_publisher(publisher, || {
        // Private setup supplies the same immutable registered replay association;
        // integration coverage separately registers a real pristine ReplayVisual.
        SESSION.with(|owner| {
            let mut owner = owner.borrow_mut();
            let owner = owner.as_mut().unwrap();
            let mut member = LocalPlayerSnapshot::new(PlayerId(1), None);
            member.gauge = BmsGauge::new(policy.clone());
            owner.snapshot.players.push(member);
            owner.pressed.push(PressedState::default());
            owner.replay_policy = Some(policy.clone());
        });
        let gauge = BmsGauge::new(policy.clone());
        let event = fixtures::report(1, 1, 1, 0).report;
        publish_replay_prefix_with_gauge(
            event.song_time,
            &event.judge_events,
            1,
            MineDamageSummary::default(),
            &gauge,
        )
        .unwrap();
        let before = snapshot();
        assert!(publish_replay_prefix_with_gauge(
            Timestamp::from_nanos(2),
            &event.judge_events,
            2,
            MineDamageSummary::default(),
            &BmsGauge::default()
        )
        .is_err());
        assert_members_equal(&snapshot(), &before);
        assert!(publish_replay_prefix_with_gauge(
            Timestamp::from_nanos(2),
            &event.judge_events,
            u32::MAX,
            MineDamageSummary::default(),
            &gauge
        )
        .is_err());
        assert_members_equal(&snapshot(), &before);
        let mut invalid = event.clone();
        invalid.hazard_events = vec![hazard(0)];
        assert!(publish_report(&invalid).is_err());
        assert_members_equal(&snapshot(), &before);
        let mut failed = gauge.clone();
        failed.observe(&[], &[hazard(1295)]).unwrap();
        let mut mines = MineDamageSummary::default();
        mines.observe(&[hazard(1295)]).unwrap();
        publish_replay_prefix_with_gauge(Timestamp::from_nanos(3), &[], 3, mines, &failed).unwrap();
        publish_pause(PauseState::Running);
        let failure = viewer.take_latest().unwrap();
        assert_eq!(failure.pressed_lanes, 0);
        assert_eq!(failure.players[0].pressed_lanes, 0);
        let before = snapshot();
        assert!(publish_replay_prefix_with_gauge(
            Timestamp::from_nanos(4),
            &event.judge_events,
            1,
            mines,
            &gauge
        )
        .is_err());
        assert_members_equal(&snapshot(), &before);
        assert!(publish_replay_prefix_with_gauge(
            Timestamp::from_nanos(4),
            &[],
            0,
            MineDamageSummary::default(),
            &failed
        )
        .is_err());
        assert_members_equal(&snapshot(), &before);
        publish_replay_prefix_with_gauge(Timestamp::from_nanos(5), &[], 1, mines, &failed).unwrap();
        publish_pause(PauseState::Paused);
        let recovered = viewer.take_latest().unwrap();
        assert_eq!(recovered.gauge, failed);
        assert_eq!(recovered.mine_damage, mines);
        assert_eq!(recovered.score.hits, 1);
        assert_eq!(recovered.pressed_lanes, 0);
        Ok(())
    })
    .unwrap();
}

#[test]
fn solo_live_custom_policy_late_refusals_discard_calculated_gauge_and_pressed_changes() {
    let policy = profile();
    let (source, chart) = fixtures::chart_fixture();
    let (publisher, viewer) = channel();
    with_publisher(publisher, || {
        publish_chart(&source, &chart).unwrap();
        prepare_native_policies(&[(PlayerId(1), &policy)]).unwrap();
        let mut seed = fixtures::report(1, 1, 1, 0).report;
        seed.bound_inputs = vec![button(7, 4, 0x11, ButtonState::Down)];
        publish_report(&seed).unwrap();
        let before = snapshot();
        SESSION.with(|owner| {
            owner.borrow_mut().as_mut().unwrap().snapshot.players[0]
                .score
                .hits = u64::MAX
        });
        let overflow = snapshot();
        let mut bad = fixtures::report(1, 2, 1, 0).report;
        bad.hazard_events = vec![hazard(1)];
        bad.bound_inputs = vec![button(8, 5, 0x12, ButtonState::Down)];
        assert!(publish_report(&bad).is_err());
        assert_members_equal(&snapshot(), &overflow);
        SESSION.with(|owner| {
            owner.borrow_mut().as_mut().unwrap().snapshot.players[0]
                .score
                .hits = before.players[0].score.hits
        });
        bad.bound_inputs = (100..4197)
            .map(|id| button(id, 4, 0x11, ButtonState::Down))
            .collect();
        assert!(publish_report(&bad).is_err());
        assert_members_equal(&snapshot(), &before);
        bad.bound_inputs.clear();
        bad.hazard_events.push(hazard(0));
        assert!(publish_report(&bad).is_err());
        assert_members_equal(&snapshot(), &before);
        let mut recovery = fixtures::report(1, 3, 1, 0).report;
        recovery.bound_inputs = vec![button(7, 4, 0x11, ButtonState::Up)];
        recovery.hazard_events = vec![hazard(1)];
        publish_report(&recovery).unwrap();
        publish_pause(PauseState::Running);
        let recovered = viewer.take_latest().unwrap();
        let mut expected = BmsGauge::new(policy);
        expected.observe(&seed.judge_events, &[]).unwrap();
        expected
            .observe(&recovery.judge_events, &recovery.hazard_events)
            .unwrap();
        assert_eq!(recovered.gauge, expected);
        assert_eq!(recovered.score.hits, 2);
        assert_eq!(recovered.mine_damage.triggered, 1);
        assert_eq!(recovered.pressed_lanes, 0);
        assert_eq!(
            recovered.recent_results,
            vec![seed.judge_events[0], recovery.judge_events[0]]
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn legacy_default_replay_replaces_live_custom_profile_only_after_whole_prefix_validation() {
    let custom = profile();
    let (source, chart) = fixtures::chart_fixture();
    let (publisher, viewer) = channel();
    with_publisher(publisher, || {
        publish_chart(&source, &chart).unwrap();
        prepare_native_policies(&[(PlayerId(1), &custom)]).unwrap();
        let report = fixtures::report(1, 1, 1, 0).report;
        let mut incoming = BmsGauge::default();
        incoming.observe(&report.judge_events, &[]).unwrap();
        assert_ne!(incoming.profile(), &custom);
        SESSION.with(|owner| {
            owner.borrow_mut().as_mut().unwrap().snapshot.players[0]
                .score
                .hits = u64::MAX;
        });
        let before = snapshot();
        assert!(publish_replay_prefix_with_gauge(
            report.song_time,
            &report.judge_events,
            1,
            MineDamageSummary::default(),
            &incoming,
        )
        .is_err());
        assert_members_equal(&snapshot(), &before);
        assert_eq!(snapshot().players[0].gauge.profile(), &custom);
        SESSION.with(|owner| {
            owner.borrow_mut().as_mut().unwrap().snapshot.players[0]
                .score
                .hits = 0;
        });
        publish_replay_prefix_with_gauge(
            report.song_time,
            &report.judge_events,
            1,
            MineDamageSummary::default(),
            &incoming,
        )
        .unwrap();
        publish_pause(PauseState::Running);
        let accepted = viewer.take_latest().unwrap();
        assert_eq!(accepted.players[0].gauge, incoming);
        assert_eq!(accepted.gauge, incoming);
        assert_eq!(accepted.score.hits, 1);
        assert_eq!(accepted.recent_results, report.judge_events);
        assert_eq!(accepted.pressed_lanes, 1);
        let next = fixtures::report(1, 2, 1, 0).report;
        incoming.observe(&next.judge_events, &[]).unwrap();
        publish_report(&next).unwrap();
        publish_pause(PauseState::Paused);
        let continued = viewer.take_latest().unwrap();
        assert_eq!(continued.players[0].gauge, incoming);
        assert_eq!(continued.gauge, incoming);
        assert_eq!(continued.score.hits, 2);
        assert_eq!(accepted.score.hits, 1);
        assert_eq!(
            accepted.players[0].gauge.profile(),
            &GaugeProfile::default()
        );
        Ok(())
    })
    .unwrap();
    let terminal = viewer.take_latest().unwrap();
    assert_eq!(terminal.status, PlayerStatus::Finished);
    assert_eq!(
        terminal.players[0].gauge.profile(),
        &GaugeProfile::default()
    );
    assert_eq!(terminal.pressed_lanes, 0);
}
