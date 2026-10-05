//! Deferred cross-owner transactions; injected numeric faults are not live proof.
use super::*;
use crate::replay_capture::LiveReplayCapture;
use beatkernel::{
    input::{
        ButtonEvent, ButtonState, CodecLimits, DeviceId, EventMeta, GameInputEvent,
        PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow, MissReason},
    replay::ReplaySession,
    time::{ClockDomainId, ClockPoint, Duration},
};
use beatkernel_bms::{ParseOptions, parse};

fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(1 << 20, 100, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn recorded(domain: u32) -> (BmsChart, ReplayFile, Vec<JudgeEvent>) {
    let source = parse(
        "#BPM 60\n#WAV01 tap.wav\n#00011:00010001\n",
        ParseOptions::default(),
    )
    .unwrap();
    let judge = JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(7),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::from_nanos(10),
        )
        .unwrap(),
    )
    .unwrap();
    let domain = ClockDomainId(domain);
    let header = LiveReplayCapture::new(&judge, domain, limits())
        .unwrap()
        .header()
        .clone();
    let mut session = ReplaySession::new(header.clone(), judge).unwrap();
    for (sequence, time, state) in [
        (0, 999_999_990, ButtonState::Down),
        (1, 1_100_000_000, ButtonState::Up),
        (2, 2_999_999_990, ButtonState::Down),
    ] {
        session
            .push_input(
                GameInputEvent {
                    game_control: source.notes[0].lane.control(),
                    physical: PhysicalInputEvent::Button(ButtonEvent {
                        meta: EventMeta::new(
                            DeviceId(3),
                            ClockPoint {
                                domain,
                                timestamp: ts(time),
                            },
                            sequence,
                        ),
                        control: PhysicalControlId::keyboard(4),
                        state,
                    }),
                },
                ts(time),
            )
            .unwrap();
    }
    session.advance_to(ts(4_000_000_000)).unwrap();
    let events = session.results().to_vec();
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|event| matches!(
        event.outcome,
        JudgeOutcome::Hit {
            grade: JudgeGrade(7),
            ..
        }
    )));
    (
        source,
        ReplayFile::new(header, session.records().to_vec()),
        events,
    )
}
fn owner(count: usize) -> (Competition, Vec<JudgeEvent>) {
    let (source, file, events) = recorded(1);
    let mut owner = Competition::new(file.header.clone(), count).unwrap();
    for index in 0..count {
        let (_, file, _) = recorded(index as u32 + 2);
        owner
            .add_replay(
                &source,
                file,
                limits(),
                OpponentKind::Other,
                format!("saved {index}"),
            )
            .unwrap();
    }
    (owner, events)
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Values {
    local: ScoreSummary,
    time: Option<Timestamp>,
    ghosts: Vec<(usize, ScoreSummary, Option<Timestamp>)>,
}
fn values(owner: &Competition) -> Values {
    Values {
        local: owner.score.clone(),
        time: owner.song_time,
        ghosts: owner
            .opponents
            .iter()
            .map(|ghost| (ghost.cursor, ghost.score.clone(), ghost.song_time))
            .collect(),
    }
}
fn scratch(owner: &Competition) -> (*const (), usize) {
    assert!(owner.prepared_updates.is_empty());
    (
        owner.prepared_updates.as_ptr().cast(),
        owner.prepared_updates.capacity(),
    )
}
fn old_score(score: &mut ScoreSummary, events: &[JudgeEvent]) -> Result<(), CompetitionError> {
    if events.is_empty() {
        return Ok(());
    }
    let mut next = score.clone();
    next.timing.observe(events)?;
    for event in events {
        match event.outcome {
            JudgeOutcome::Hit { grade, .. } => {
                next.hits = next
                    .hits
                    .checked_add(1)
                    .ok_or(CompetitionError::ScoreOverflow)?;
                next.combo = next
                    .combo
                    .checked_add(1)
                    .ok_or(CompetitionError::ScoreOverflow)?;
                next.max_combo = next.max_combo.max(next.combo);
                let count = next.grades.entry(grade.0).or_default();
                *count = count
                    .checked_add(1)
                    .ok_or(CompetitionError::ScoreOverflow)?;
            }
            JudgeOutcome::Miss { .. } => {
                next.misses = next
                    .misses
                    .checked_add(1)
                    .ok_or(CompetitionError::ScoreOverflow)?;
                next.combo = 0;
            }
        }
    }
    *score = next;
    Ok(())
}
// Fresh Vec and cloned maps reproduce the previous complete transaction, using
// only borrowed recorded events, never the production planning/prefix helpers.
fn old_transaction(
    owner: &Competition,
    events: &[JudgeEvent],
    time: Timestamp,
    rebuild: bool,
) -> Result<Values, CompetitionError> {
    if !rebuild && owner.song_time.is_some_and(|previous| time < previous) {
        return Err(CompetitionError::TimeRegression);
    }
    let mut local = if rebuild {
        ScoreSummary::default()
    } else {
        owner.score.clone()
    };
    old_score(&mut local, events)?;
    let mut ghosts = Vec::new();
    for ghost in &owner.opponents {
        let cursor = ghost
            .batches
            .iter()
            .take_while(|batch| batch.song_time <= time)
            .count();
        let forward = cursor >= ghost.cursor;
        let mut score = if forward {
            ghost.score.clone()
        } else {
            ScoreSummary::default()
        };
        let start = if forward { ghost.cursor } else { 0 };
        for batch in &ghost.batches[start..cursor] {
            old_score(&mut score, &batch.events)?;
        }
        ghosts.push((cursor, score, Some(time)));
    }
    Ok(Values {
        local,
        time: Some(time),
        ghosts,
    })
}
#[derive(Debug, PartialEq, Eq)]
enum Failure {
    Score,
    Timing,
    Time,
}
fn failure<T>(result: Result<T, CompetitionError>) -> Failure {
    match result {
        Err(CompetitionError::ScoreOverflow) => Failure::Score,
        Err(CompetitionError::TimingOverflow) => Failure::Timing,
        Err(CompetitionError::TimeRegression) => Failure::Time,
        Err(other) => panic!("unexpected transaction error {other:?}"),
        Ok(_) => panic!("expected refusal"),
    }
}

#[test]
fn nonempty_local_and_advancing_genuine_ghost_reports_retain_existing_grade_nodes_and_scratch() {
    let (mut owner, events) = owner(3);
    owner.observe(&events[..1], ts(999_999_990)).unwrap();
    let local = owner.score.grades.get(&7).unwrap() as *const u64;
    let ghost_nodes: Vec<_> = owner
        .opponents
        .iter()
        .map(|ghost| ghost.score.grades.get(&7).unwrap() as *const u64)
        .collect();
    let storage = scratch(&owner);
    let expected = old_transaction(&owner, &events[1..], ts(2_999_999_990), false).unwrap();
    owner.observe(&events[1..], ts(2_999_999_990)).unwrap();
    assert_eq!(values(&owner), expected);
    assert_eq!(owner.score.grades.get(&7).unwrap() as *const u64, local);
    for (ghost, pointer) in owner.opponents.iter().zip(ghost_nodes) {
        assert_eq!(ghost.score.grades.get(&7).unwrap() as *const u64, pointer);
        assert_eq!(ghost.score.grades.get(&7), Some(&2));
    }
    assert_eq!(scratch(&owner), storage);
}

#[test]
fn ordered_reference_covers_forward_backward_reset_late_admission_and_exact_frontier_times() {
    let (mut owner, events) = owner(2);
    for (time, local) in [
        (0, &[][..]),
        (999_999_990, &events[..1]),
        (1_500_000_000, &[][..]),
        (2_999_999_990, &events[1..]),
        (2_999_999_990, &[][..]),
        (604_800_000_000_000, &[][..]),
        (9_007_199_254_740_993, &[][..]),
    ] {
        let expected = old_transaction(&owner, local, ts(time), false).unwrap();
        owner.observe(local, ts(time)).unwrap();
        assert_eq!(values(&owner), expected);
    }
    for (time, local) in [
        (0, &[][..]),
        (999_999_990, &events[..1]),
        (2_999_999_990, &events[..]),
    ] {
        let expected = old_transaction(&owner, local, ts(time), true).unwrap();
        owner.rebuild(local, ts(time)).unwrap();
        assert_eq!(values(&owner), expected);
    }
    let storage = scratch(&owner);
    owner.reset();
    assert_eq!(scratch(&owner), storage);
    let expected = old_transaction(&owner, &[], ts(2_999_999_990), false).unwrap();
    owner.observe(&[], ts(2_999_999_990)).unwrap();
    assert_eq!(values(&owner), expected);
    let (source, file, _) = recorded(98);
    let mut late = Competition::new(file.header.clone(), 1).unwrap();
    late.observe(&events, ts(9_007_199_254_740_993)).unwrap();
    late.add_replay(&source, file, limits(), OpponentKind::Own, "late")
        .unwrap();
    assert_eq!(late.opponents[0].score, late.score);
    assert_eq!(late.opponents[0].song_time, late.song_time);
    assert_eq!(late.opponents[0].recorded_until, Some(ts(4_000_000_000)));
    assert_eq!(late.opponents[0].score.misses, 0);
}

#[test]
fn later_owner_fault_preserves_every_live_node_and_clears_staging_for_genuine_retry() {
    for timing in [false, true] {
        let (mut owner, events) = owner(3);
        owner.observe(&events[..1], ts(999_999_990)).unwrap();
        let nodes: Vec<_> = owner
            .opponents
            .iter()
            .map(|ghost| ghost.score.grades.get(&7).unwrap() as *const u64)
            .collect();
        let local = owner.score.grades.get(&7).unwrap() as *const u64;
        let healthy = owner.opponents[2].score.clone();
        if timing {
            owner.opponents[2].score.timing = TimingSummary::exhausted_for_fixture();
        } else {
            owner.opponents[2].score.hits = u64::MAX;
        }
        let before = values(&owner);
        let storage = scratch(&owner);
        let expected_error = failure(old_transaction(
            &owner,
            &events[1..],
            ts(2_999_999_990),
            false,
        ));
        assert_eq!(
            failure(owner.observe(&events[1..], ts(2_999_999_990))),
            expected_error
        );
        assert_eq!(values(&owner), before);
        assert_eq!(scratch(&owner), storage);
        assert_eq!(owner.score.grades.get(&7).unwrap() as *const u64, local);
        for (ghost, pointer) in owner.opponents.iter().zip(nodes) {
            assert_eq!(ghost.score.grades.get(&7).unwrap() as *const u64, pointer);
        }
        owner.opponents[2].score = healthy;
        let expected = old_transaction(&owner, &events[1..], ts(2_999_999_990), false).unwrap();
        owner.observe(&events[1..], ts(2_999_999_990)).unwrap();
        assert_eq!(values(&owner), expected);
        assert_eq!(scratch(&owner), storage);
    }
}

#[test]
fn same_grade_hits_across_two_batches_use_cumulative_exact_limit_and_refuse_atomically() {
    for initial in [u64::MAX - 2, u64::MAX - 1] {
        let (mut owner, _) = owner(2);
        owner.opponents[1].score.grades.insert(7, initial);
        owner.opponents[1].score.grades.insert(u32::MAX, u64::MAX);
        let before = values(&owner);
        let storage = scratch(&owner);
        let reference = old_transaction(&owner, &[], ts(2_999_999_990), false);
        if initial == u64::MAX - 2 {
            owner.observe(&[], ts(2_999_999_990)).unwrap();
            assert_eq!(values(&owner), reference.unwrap());
            assert_eq!(owner.opponents[1].score.grades.get(&7), Some(&u64::MAX));
            assert_eq!(
                owner.opponents[1].score.grades.get(&u32::MAX),
                Some(&u64::MAX)
            );
        } else {
            assert_eq!(failure(reference), Failure::Score);
            assert_eq!(
                failure(owner.observe(&[], ts(2_999_999_990))),
                Failure::Score
            );
            assert_eq!(values(&owner), before);
        }
        assert_eq!(scratch(&owner), storage);
    }
}

#[test]
fn near_limit_cumulative_grade_check_uses_matching_hits_not_all_prefix_hits() {
    let (mut owner, _) = owner(1);
    // Fault injection changes an admitted event's opaque grade, not its time.
    let event = &mut owner.opponents[0].batches[1].events[0];
    let JudgeOutcome::Hit { delta, .. } = event.outcome else {
        panic!("genuine hit required")
    };
    event.outcome = JudgeOutcome::Hit {
        grade: JudgeGrade(u32::MAX),
        delta,
    };
    owner.opponents[0].score.grades.insert(7, u64::MAX - 1);
    owner.opponents[0]
        .score
        .grades
        .insert(u32::MAX, u64::MAX - 1);
    owner.opponents[0].score.grades.insert(0, u64::MAX);
    let expected = old_transaction(&owner, &[], ts(2_999_999_990), false).unwrap();
    owner.observe(&[], ts(2_999_999_990)).unwrap();
    assert_eq!(values(&owner), expected);
    for grade in [0, 7, u32::MAX] {
        assert_eq!(owner.opponents[0].score.grades.get(&grade), Some(&u64::MAX));
    }
}

#[test]
fn earlier_batch_score_failure_wins_later_timing_failure_but_same_batch_timing_wins() {
    for first_custom in [true, false] {
        let (mut owner, _) = owner(1);
        owner.opponents[0].score.hits = u64::MAX;
        owner.opponents[0].score.timing = TimingSummary::exhausted_for_fixture();
        if first_custom {
            owner.opponents[0].batches[0].events[0].stage = JudgeStage::Custom(0);
        }
        let before = values(&owner);
        let expected = if first_custom {
            Failure::Score
        } else {
            Failure::Timing
        };
        assert_eq!(
            failure(old_transaction(&owner, &[], ts(2_999_999_990), false)),
            expected
        );
        assert_eq!(failure(owner.observe(&[], ts(2_999_999_990))), expected);
        assert_eq!(values(&owner), before);
        assert!(owner.prepared_updates.is_empty());
    }
}

#[test]
fn ordered_miss_batches_reset_exhausted_combo_and_keep_earlier_score_error_priority() {
    for exhausted_misses in [false, true] {
        let (mut owner, _) = owner(1);
        owner.opponents[0].batches[0].events[0].outcome = JudgeOutcome::Miss {
            reason: MissReason::HeadTimeout,
        };
        owner.opponents[0].score.combo = u64::MAX;
        owner.opponents[0].score.max_combo = u64::MAX;
        if exhausted_misses {
            owner.opponents[0].score.misses = u64::MAX;
            owner.opponents[0].score.timing = TimingSummary::exhausted_for_fixture();
        }
        let before = values(&owner);
        let reference = old_transaction(&owner, &[], ts(2_999_999_990), false);
        if exhausted_misses {
            assert_eq!(failure(reference), Failure::Score);
            assert_eq!(
                failure(owner.observe(&[], ts(2_999_999_990))),
                Failure::Score
            );
            assert_eq!(values(&owner), before);
        } else {
            owner.observe(&[], ts(2_999_999_990)).unwrap();
            assert_eq!(values(&owner), reference.unwrap());
            assert_eq!(
                (
                    owner.opponents[0].score.hits,
                    owner.opponents[0].score.misses,
                    owner.opponents[0].score.combo,
                    owner.opponents[0].score.max_combo
                ),
                (1, 1, 1, u64::MAX)
            );
        }
        assert!(owner.prepared_updates.is_empty());
    }
}
