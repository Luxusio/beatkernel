//! Deferred independent scoring-policy and retained-storage regressions.
use super::{CompetitionError, ScoreSummary};
use crate::timing::TimingSummary;
use beatkernel::{
    chart::ObjectId,
    judge::{JudgeEvent, JudgeGrade, JudgeOutcome, JudgeStage, MissReason},
    time::{Duration, Timestamp},
};

fn hit(grade: u32, delta: i64, stage: JudgeStage) -> JudgeEvent {
    JudgeEvent {
        object: ObjectId(1),
        stage,
        outcome: JudgeOutcome::Hit {
            grade: JudgeGrade(grade),
            delta: Duration::from_nanos(delta),
        },
        at: Timestamp::ZERO,
        input: None,
    }
}
fn miss() -> JudgeEvent {
    JudgeEvent {
        object: ObjectId(2),
        stage: JudgeStage::HoldTail,
        outcome: JudgeOutcome::Miss {
            reason: MissReason::TailTimeout,
        },
        at: Timestamp::ZERO,
        input: None,
    }
}

// The former ordered transaction, kept independently of production preflight.
fn ordered_reference(
    score: &mut ScoreSummary,
    events: &[JudgeEvent],
) -> Result<(), CompetitionError> {
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
#[derive(Debug, PartialEq, Eq)]
enum Disposition {
    Accepted,
    ScoreOverflow,
    TimingOverflow,
}
fn disposition(result: Result<(), CompetitionError>) -> Disposition {
    match result {
        Ok(()) => Disposition::Accepted,
        Err(CompetitionError::ScoreOverflow) => Disposition::ScoreOverflow,
        Err(CompetitionError::TimingOverflow) => Disposition::TimingOverflow,
        Err(other) => panic!("unexpected scoring error: {other:?}"),
    }
}
fn compare(score: &mut ScoreSummary, reference: &mut ScoreSummary, batch: &[JudgeEvent]) {
    assert_eq!(
        disposition(score.observe(batch)),
        disposition(ordered_reference(reference, batch))
    );
    assert_eq!(score, reference);
}

#[test]
fn deterministic_ordered_batches_match_legacy_transaction_including_split_reports() {
    let mut seed = 0x729a_4bc1_059e_386du64;
    let mut events = Vec::new();
    for _ in 0..257 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let grade = [0, 7, u32::MAX][(seed % 3) as usize];
        let delta = [i64::MIN, -17, 0, 23, i64::MAX][((seed >> 5) % 5) as usize];
        let stage = [
            JudgeStage::Instant,
            JudgeStage::HoldHead,
            JudgeStage::HoldTail,
            JudgeStage::Custom(u32::MAX),
        ][((seed >> 11) % 4) as usize];
        events.push(if seed & 7 == 0 {
            miss()
        } else {
            hit(grade, delta, stage)
        });
    }
    for width in [1, 2, 7, 64, 257] {
        for boundary in 0..6 {
            let mut score = ScoreSummary::default();
            match boundary {
                1 => score.hits = u64::MAX - 3,
                2 => score.misses = u64::MAX - 1,
                3 => score.combo = u64::MAX - 2,
                4 => {
                    score.grades.insert(7, u64::MAX - 1);
                }
                5 => score.timing = TimingSummary::exhausted_for_fixture(),
                _ => {}
            }
            let mut reference = score.clone();
            for batch in events.chunks(width) {
                compare(&mut score, &mut reference, batch);
            }
        }
    }
    let mut whole = ScoreSummary::default();
    whole.observe(&events).unwrap();
    let mut split = ScoreSummary::default();
    for batch in events.chunks(7) {
        split.observe(batch).unwrap();
    }
    assert_eq!(whole, split);
}

#[test]
fn hold_custom_and_miss_order_has_independent_exact_score_and_timing_expectations() {
    let events = [
        hit(0, i64::MIN, JudgeStage::HoldHead),
        hit(u32::MAX, i64::MAX, JudgeStage::HoldTail),
        hit(0, i64::MAX, JudgeStage::Custom(91)),
        miss(),
        hit(7, 0, JudgeStage::Instant),
        miss(),
    ];
    let mut score = ScoreSummary::default();
    score.observe(&events).unwrap();
    assert_eq!(
        (score.hits, score.misses, score.combo, score.max_combo),
        (4, 2, 0, 3)
    );
    assert_eq!(score.grades.get(&0), Some(&2));
    assert_eq!(score.grades.get(&7), Some(&1));
    assert_eq!(score.grades.get(&u32::MAX), Some(&1));
    assert_eq!(
        (
            score.timing.count(),
            score.timing.early(),
            score.timing.late(),
            score.timing.exact()
        ),
        (3, 1, 1, 1)
    );
    assert_eq!(score.timing.mean_ns(), Some(0));
    assert_eq!(
        score.timing.mean_absolute_ns(),
        Some(6_148_914_691_236_517_205)
    );
    assert_eq!(score.timing.last_ns(), Some(0));
    assert_eq!(score.timing.min_ns(), Some(i64::MIN));
    assert_eq!(score.timing.max_ns(), Some(i64::MAX));
}

#[test]
fn global_counter_limits_are_exact_and_failed_batches_leave_every_field_unchanged() {
    for field in 0..3 {
        let mut score = ScoreSummary::default();
        match field {
            0 => score.hits = u64::MAX,
            1 => score.misses = u64::MAX,
            _ => score.combo = u64::MAX,
        }
        let before = score.clone();
        let events = if field == 1 {
            vec![hit(91, -2, JudgeStage::Instant), miss()]
        } else {
            vec![hit(91, -2, JudgeStage::Instant)]
        };
        assert_eq!(
            disposition(score.observe(&events)),
            Disposition::ScoreOverflow
        );
        assert_eq!(score, before);
        assert!(!score.grades.contains_key(&91));
    }
    let mut score = ScoreSummary {
        hits: u64::MAX - 1,
        misses: u64::MAX - 1,
        combo: u64::MAX,
        max_combo: u64::MAX,
        ..Default::default()
    };
    score
        .observe(&[miss(), hit(0, 0, JudgeStage::Custom(0))])
        .unwrap();
    assert_eq!(
        (score.hits, score.misses, score.combo, score.max_combo),
        (u64::MAX, u64::MAX, 1, u64::MAX)
    );
    let before = score.clone();
    assert_eq!(
        disposition(score.observe(&[hit(7, 0, JudgeStage::Custom(0))])),
        Disposition::ScoreOverflow
    );
    assert_eq!(score, before);
}

#[test]
fn timing_failure_precedes_score_failure_and_empty_exhausted_observation_is_noop() {
    let mut score = ScoreSummary {
        hits: u64::MAX,
        misses: u64::MAX,
        combo: u64::MAX,
        max_combo: u64::MAX,
        timing: TimingSummary::exhausted_for_fixture(),
        ..Default::default()
    };
    score.grades.insert(0, u64::MAX);
    let before = score.clone();
    score.observe(&[]).unwrap();
    assert_eq!(score, before);
    assert_eq!(
        disposition(score.observe(&[hit(0, 0, JudgeStage::Instant)])),
        Disposition::TimingOverflow
    );
    assert_eq!(score, before);
    assert_eq!(
        disposition(score.observe(&[hit(0, 0, JudgeStage::Custom(1))])),
        Disposition::ScoreOverflow
    );
    assert_eq!(score, before);
    let mut custom = ScoreSummary {
        timing: TimingSummary::exhausted_for_fixture(),
        ..Default::default()
    };
    custom
        .observe(&[hit(u32::MAX, i64::MIN, JudgeStage::Custom(1)), miss()])
        .unwrap();
    assert_eq!((custom.hits, custom.misses, custom.combo), (1, 1, 0));
    assert_eq!(custom.timing, before.timing);
}

#[test]
fn grade_preflight_counts_only_actual_matches_and_accepts_exact_boundaries() {
    let mut score = ScoreSummary::default();
    score.grades.insert(0, u64::MAX);
    score.grades.insert(7, u64::MAX - 1);
    score.grades.insert(u32::MAX, u64::MAX - 2);
    score
        .observe(&[
            hit(7, 0, JudgeStage::Instant),
            hit(91, 0, JudgeStage::Custom(9)),
            hit(u32::MAX, 1, JudgeStage::HoldHead),
            hit(u32::MAX, -1, JudgeStage::HoldTail),
        ])
        .unwrap();
    assert_eq!(score.grades.get(&0), Some(&u64::MAX));
    assert_eq!(score.grades.get(&7), Some(&u64::MAX));
    assert_eq!(score.grades.get(&u32::MAX), Some(&u64::MAX));
    assert_eq!(score.grades.get(&91), Some(&1));
    assert_eq!(score.hits, 4);
}

#[test]
fn late_grade_overflow_cannot_insert_a_new_grade_or_commit_earlier_timing_and_counts() {
    let mut score = ScoreSummary::default();
    score.grades.insert(7, u64::MAX - 1);
    let before = score.clone();
    let events = [
        hit(91, -3, JudgeStage::Instant),
        hit(7, 1, JudgeStage::HoldHead),
        miss(),
        hit(7, 2, JudgeStage::HoldTail),
    ];
    assert_eq!(
        disposition(score.observe(&events)),
        Disposition::ScoreOverflow
    );
    assert_eq!(score, before);
    assert!(!score.grades.contains_key(&91));
    let mut reference = before.clone();
    assert_eq!(
        disposition(ordered_reference(&mut reference, &events)),
        Disposition::ScoreOverflow
    );
    assert_eq!(score, reference);
}

#[test]
fn existing_grade_value_addresses_survive_successful_reports_without_new_grades() {
    let mut score = ScoreSummary::default();
    for grade in 0..128 {
        score.grades.insert(grade, grade as u64);
    }
    score.grades.insert(u32::MAX, 1);
    let addresses: Vec<_> = score
        .grades
        .iter()
        .map(|(&grade, count)| (grade, count as *const u64))
        .collect();
    let mut reference = score.clone();
    for _ in 0..16 {
        let events = [
            hit(0, -1, JudgeStage::Instant),
            hit(127, 1, JudgeStage::HoldTail),
            miss(),
            hit(u32::MAX, i64::MIN, JudgeStage::Custom(1)),
        ];
        compare(&mut score, &mut reference, &events);
        for &(grade, pointer) in &addresses {
            assert_eq!(score.grades.get(&grade).unwrap() as *const u64, pointer);
        }
    }
    // Address stability pins retained existing storage, not global allocator freedom.
}

fn local_competition(capacity: usize) -> super::Competition {
    use crate::replay_capture::LiveReplayCapture;
    use beatkernel::{
        input::CodecLimits,
        judge::{JudgeEngine, JudgeProfile, JudgeWindow},
        replay::codec::ReplayCodecLimits,
        time::ClockDomainId,
    };
    use beatkernel_bms::{ParseOptions, parse};
    let source = parse(
        "#BPM 60\n#WAV01 tap.wav\n#00011:01\n",
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
    let limits =
        ReplayCodecLimits::new(1 << 20, 100, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap();
    let capture = LiveReplayCapture::new(&judge, ClockDomainId(93), limits).unwrap();
    super::Competition::new(capture.header().clone(), capacity).unwrap()
}

#[test]
fn actual_empty_opponent_owner_retains_grade_storage_and_publishes_exact_empty_report_time() {
    for capacity in [0, 8] {
        let mut competition = local_competition(capacity);
        let week = 604_800_000_000_000i64;
        competition
            .observe(
                &[
                    hit(0, -1, JudgeStage::Instant),
                    hit(u32::MAX, 1, JudgeStage::HoldTail),
                ],
                Timestamp::from_nanos(week),
            )
            .unwrap();
        let pointers: Vec<_> = competition
            .score()
            .grades
            .iter()
            .map(|(&grade, value)| (grade, value as *const u64))
            .collect();
        let precise = 9_007_199_254_740_993i64;
        competition
            .observe(
                &[
                    hit(0, 0, JudgeStage::Custom(7)),
                    miss(),
                    hit(u32::MAX, 0, JudgeStage::Instant),
                ],
                Timestamp::from_nanos(precise),
            )
            .unwrap();
        for &(grade, pointer) in &pointers {
            assert_eq!(
                competition.score().grades.get(&grade).unwrap() as *const u64,
                pointer
            );
        }
        assert_eq!(
            competition.song_time(),
            Some(Timestamp::from_nanos(precise))
        );
        let before = competition.score().clone();
        competition
            .observe(&[], Timestamp::from_nanos(precise + 1))
            .unwrap();
        assert_eq!(
            competition.song_time(),
            Some(Timestamp::from_nanos(precise + 1))
        );
        assert_eq!(competition.score(), &before);
        assert!(competition.opponents().is_empty());
        assert_eq!(competition.remaining_opponent_capacity(), capacity);
        for &(grade, pointer) in &pointers {
            assert_eq!(
                competition.score().grades.get(&grade).unwrap() as *const u64,
                pointer
            );
        }
    }
}

#[test]
fn actual_empty_opponent_owner_time_guard_precedes_numeric_failure_and_failure_preserves_time() {
    for capacity in [0, 8] {
        let mut competition = local_competition(capacity);
        let accepted_time = Timestamp::from_nanos(9_007_199_254_740_993);
        competition
            .observe(&[hit(7, 0, JudgeStage::Instant)], accepted_time)
            .unwrap();
        // Public summary states permit exhaustion; child access installs those
        // states on the actual owner without manufacturing a completion result.
        competition.score.hits = u64::MAX;
        competition.score.timing = TimingSummary::exhausted_for_fixture();
        let before = competition.score().clone();
        let event = hit(7, 0, JudgeStage::Instant);
        assert!(matches!(
            competition.observe(
                &[event],
                Timestamp::from_nanos(accepted_time.as_nanos() - 1)
            ),
            Err(CompetitionError::TimeRegression)
        ));
        assert_eq!(competition.score(), &before);
        assert_eq!(competition.song_time(), Some(accepted_time));
        assert!(matches!(
            competition.observe(
                &[event],
                Timestamp::from_nanos(accepted_time.as_nanos() + 1)
            ),
            Err(CompetitionError::TimingOverflow)
        ));
        assert_eq!(competition.score(), &before);
        assert_eq!(competition.song_time(), Some(accepted_time));
        assert!(matches!(
            competition.observe(
                &[hit(7, 0, JudgeStage::Custom(9))],
                Timestamp::from_nanos(accepted_time.as_nanos() + 2)
            ),
            Err(CompetitionError::ScoreOverflow)
        ));
        assert_eq!(competition.score(), &before);
        assert_eq!(competition.song_time(), Some(accepted_time));
        competition
            .observe(&[], Timestamp::from_nanos(accepted_time.as_nanos() + 3))
            .unwrap();
        assert_eq!(competition.score(), &before);
        assert_eq!(
            competition.song_time(),
            Some(Timestamp::from_nanos(accepted_time.as_nanos() + 3))
        );
    }
}
