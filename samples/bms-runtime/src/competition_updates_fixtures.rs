//! Deferred actual-owner transactional and retained-preparation regressions.
use super::*;
use crate::replay_capture::LiveReplayCapture;
use beatkernel::{
    input::{
        ButtonEvent, ButtonState, CodecLimits, DeviceId, EventMeta, GameInputEvent,
        PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeGrade, JudgeProfile, JudgeWindow},
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
fn recording(domain: u32, complete: bool) -> (BmsChart, ReplayFile, Vec<JudgeEvent>) {
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
    let input = GameInputEvent {
        game_control: source.notes[0].lane.control(),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(
                DeviceId(3),
                ClockPoint {
                    domain,
                    timestamp: ts(99),
                },
                0,
            ),
            control: PhysicalControlId::keyboard(4),
            state: ButtonState::Down,
        }),
    };
    session.push_input(input, ts(999_999_990)).unwrap();
    if complete {
        session.advance_to(ts(3_000_000_000)).unwrap();
    }
    let events = session.results().to_vec();
    (
        source,
        ReplayFile::new(header, session.records().to_vec()),
        events,
    )
}
fn owner(count: usize) -> (Competition, Vec<JudgeEvent>) {
    let (source, file, events) = recording(1, true);
    let mut competition = Competition::new(file.header.clone(), count).unwrap();
    for index in 0..count {
        let (_, remote, _) = recording(index as u32 + 2, index % 2 == 0);
        competition
            .add_replay(
                &source,
                remote,
                limits(),
                OpponentKind::Other,
                format!("record {index}"),
            )
            .unwrap();
    }
    (competition, events)
}

// Independent former ordered score transaction; production prefix/update are
// deliberately not called by this reference.
fn old_score(score: &mut ScoreSummary, events: &[JudgeEvent]) -> Result<(), CompetitionError> {
    if events.is_empty() {
        return Ok(());
    }
    let mut candidate = score.clone();
    candidate.timing.observe(events)?;
    for event in events {
        match event.outcome {
            JudgeOutcome::Hit { grade, .. } => {
                candidate.hits = candidate
                    .hits
                    .checked_add(1)
                    .ok_or(CompetitionError::ScoreOverflow)?;
                candidate.combo = candidate
                    .combo
                    .checked_add(1)
                    .ok_or(CompetitionError::ScoreOverflow)?;
                candidate.max_combo = candidate.max_combo.max(candidate.combo);
                let count = candidate.grades.entry(grade.0).or_default();
                *count = count
                    .checked_add(1)
                    .ok_or(CompetitionError::ScoreOverflow)?;
            }
            JudgeOutcome::Miss { .. } => {
                candidate.misses = candidate
                    .misses
                    .checked_add(1)
                    .ok_or(CompetitionError::ScoreOverflow)?;
                candidate.combo = 0;
            }
        }
    }
    *score = candidate;
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct GhostValue {
    cursor: usize,
    score: ScoreSummary,
    time: Option<Timestamp>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Values {
    score: ScoreSummary,
    time: Option<Timestamp>,
    ghosts: Vec<GhostValue>,
}
fn values(owner: &Competition) -> Values {
    Values {
        score: owner.score.clone(),
        time: owner.song_time,
        ghosts: owner
            .opponents
            .iter()
            .map(|ghost| GhostValue {
                cursor: ghost.cursor,
                score: ghost.score.clone(),
                time: ghost.song_time,
            })
            .collect(),
    }
}
struct OldTransaction {
    values: Values,
    batches: Vec<Vec<(Timestamp, Vec<JudgeEvent>)>>,
}
impl OldTransaction {
    fn new(owner: &Competition) -> Self {
        Self {
            values: values(owner),
            batches: owner
                .opponents
                .iter()
                .map(|ghost| {
                    ghost
                        .batches
                        .iter()
                        .map(|batch| (batch.song_time, batch.events.clone()))
                        .collect()
                })
                .collect(),
        }
    }
    fn update(
        &mut self,
        events: &[JudgeEvent],
        time: Timestamp,
        rebuild: bool,
    ) -> Result<(), CompetitionError> {
        if !rebuild && self.values.time.is_some_and(|previous| time < previous) {
            return Err(CompetitionError::TimeRegression);
        }
        let mut local = if rebuild {
            ScoreSummary::default()
        } else {
            self.values.score.clone()
        };
        old_score(&mut local, events)?;
        let mut candidates = Vec::new();
        for (old, batches) in self.values.ghosts.iter().zip(&self.batches) {
            let end = batches.iter().take_while(|(at, _)| *at <= time).count();
            let forward = end >= old.cursor;
            let mut score = if forward {
                old.score.clone()
            } else {
                ScoreSummary::default()
            };
            let start = if forward { old.cursor } else { 0 };
            for (_, events) in &batches[start..end] {
                old_score(&mut score, events)?;
            }
            candidates.push(GhostValue {
                cursor: end,
                score,
                time: Some(time),
            });
        }
        self.values = Values {
            score: local,
            time: Some(time),
            ghosts: candidates,
        };
        Ok(())
    }
}
fn scratch(owner: &Competition) -> (*const (usize, Option<ScoreSummary>), usize) {
    assert!(owner.prepared_updates.is_empty());
    (
        owner.prepared_updates.as_ptr(),
        owner.prepared_updates.capacity(),
    )
}

#[test]
fn genuine_multiple_records_match_old_transaction_forward_equal_gaps_frontier_and_backward_rebuild()
{
    let (mut competition, events) = owner(3);
    let mut reference = OldTransaction::new(&competition);
    for (time, local) in [
        (0, &[][..]),
        (999_999_989, &[][..]),
        (999_999_990, &events[..1]),
        (999_999_990, &[][..]),
        (2_000_000_000, &[][..]),
        (3_000_000_000, &events[1..]),
        (604_800_000_000_000, &[][..]),
        (9_007_199_254_740_993, &[][..]),
    ] {
        reference.update(local, ts(time), false).unwrap();
        competition.observe(local, ts(time)).unwrap();
        assert_eq!(values(&competition), reference.values);
        assert!(competition.prepared_updates.is_empty());
    }
    for (time, local) in [
        (999_999_989, &[][..]),
        (999_999_990, &events[..1]),
        (3_000_000_000, &events[..]),
    ] {
        reference.update(local, ts(time), true).unwrap();
        competition.rebuild(local, ts(time)).unwrap();
        assert_eq!(values(&competition), reference.values);
    }
}

#[test]
fn preparation_storage_is_cold_sized_by_admitted_members_and_retained_across_updates_and_reset() {
    let (_, file, _) = recording(1, true);
    let mut cold = Competition::new(file.header, usize::MAX).unwrap();
    assert_eq!(cold.prepared_updates.capacity(), 0);
    cold.observe(&[], ts(0)).unwrap();
    assert_eq!(cold.prepared_updates.capacity(), 0);
    let (mut competition, events) = owner(4);
    let storage = scratch(&competition);
    assert!(storage.1 >= 4);
    for time in [
        0,
        999_999_990,
        2_000_000_000,
        3_000_000_000,
        9_007_199_254_740_993,
    ] {
        competition.observe(&[], ts(time)).unwrap();
        assert_eq!(scratch(&competition), storage);
    }
    competition.rebuild(&events[..1], ts(999_999_990)).unwrap();
    assert_eq!(scratch(&competition), storage);
    competition.reset();
    assert_eq!(scratch(&competition), storage);
    assert_eq!(competition.song_time(), None);
    for ghost in &competition.opponents {
        assert_eq!((ghost.cursor, ghost.song_time), (0, None));
        assert_eq!(ghost.score, ScoreSummary::default());
    }
    let mut reference = OldTransaction::new(&competition);
    reference.update(&[], ts(3_000_000_000), false).unwrap();
    competition.observe(&[], ts(3_000_000_000)).unwrap();
    assert_eq!(values(&competition), reference.values);
    assert_eq!(scratch(&competition), storage);
}

#[test]
fn unchanged_prefixes_and_empty_local_reports_retain_grade_values_while_times_advance() {
    let (mut competition, events) = owner(2);
    competition.observe(&events[..1], ts(999_999_990)).unwrap();
    let local = competition.score.grades.get(&7).unwrap() as *const u64;
    let ghosts: Vec<_> = competition
        .opponents
        .iter()
        .map(|ghost| ghost.score.grades.get(&7).unwrap() as *const u64)
        .collect();
    for time in [999_999_990, 1_000_000_000, 2_999_999_999] {
        competition.observe(&[], ts(time)).unwrap();
        assert_eq!(
            competition.score.grades.get(&7).unwrap() as *const u64,
            local
        );
        for (ghost, pointer) in competition.opponents.iter().zip(&ghosts) {
            assert_eq!(ghost.score.grades.get(&7).unwrap() as *const u64, *pointer);
            assert_eq!(ghost.song_time, Some(ts(time)));
        }
    }
    competition.observe(&[], ts(3_000_000_000)).unwrap();
    assert_eq!(
        competition.score.grades.get(&7).unwrap() as *const u64,
        local
    );
    assert_eq!(competition.opponents[0].score.misses, 1);
    assert_eq!(competition.opponents[1].score.misses, 0);
    let ghosts: Vec<_> = competition
        .opponents
        .iter()
        .map(|ghost| ghost.score.grades.get(&7).unwrap() as *const u64)
        .collect();
    competition.observe(&[], ts(9_007_199_254_740_993)).unwrap();
    for (ghost, pointer) in competition.opponents.iter().zip(&ghosts) {
        assert_eq!(ghost.score.grades.get(&7).unwrap() as *const u64, *pointer);
    }
    assert_eq!(
        competition.opponents[1].recorded_until,
        Some(ts(999_999_990))
    );
    assert_eq!(competition.opponents[1].score.misses, 0);
}

#[test]
fn later_opponent_score_or_timing_failure_discards_all_staging_and_retry_is_unpoisoned() {
    for timing in [false, true] {
        let (mut competition, events) = owner(3);
        if timing {
            competition.opponents[2].score.timing = TimingSummary::exhausted_for_fixture();
        } else {
            competition.opponents[2].score.hits = u64::MAX;
        }
        let before = values(&competition);
        let storage = scratch(&competition);
        let result = competition.observe(&events[..1], ts(999_999_990));
        if timing {
            assert!(matches!(result, Err(CompetitionError::TimingOverflow)));
        } else {
            assert!(matches!(result, Err(CompetitionError::ScoreOverflow)));
        }
        assert_eq!(values(&competition), before);
        assert_eq!(scratch(&competition), storage);
        competition.opponents[2].score = ScoreSummary::default();
        let mut reference = OldTransaction::new(&competition);
        reference
            .update(&events[..1], ts(999_999_990), false)
            .unwrap();
        competition.observe(&events[..1], ts(999_999_990)).unwrap();
        assert_eq!(values(&competition), reference.values);
        assert_eq!(scratch(&competition), storage);
    }
}

#[test]
fn local_failure_and_backward_guard_preserve_all_ghosts_and_reusable_storage() {
    let (mut competition, events) = owner(2);
    competition.observe(&events[..1], ts(999_999_990)).unwrap();
    competition.score.hits = u64::MAX;
    let before = values(&competition);
    let storage = scratch(&competition);
    assert!(matches!(
        competition.observe(&events[..1], ts(0)),
        Err(CompetitionError::TimeRegression)
    ));
    assert_eq!(values(&competition), before);
    assert!(matches!(
        competition.observe(&events[..1], ts(3_000_000_000)),
        Err(CompetitionError::ScoreOverflow)
    ));
    assert_eq!(values(&competition), before);
    assert_eq!(scratch(&competition), storage);
    competition.observe(&[], ts(3_000_000_000)).unwrap();
    assert_eq!(competition.score.hits, u64::MAX);
    assert_eq!(competition.opponents[0].score.misses, 1);
    assert_eq!(scratch(&competition), storage);
}

#[test]
fn late_admission_uses_exact_recorded_prefix_and_existing_storage_then_rebuilds_normally() {
    let (source, file, events) = recording(1, true);
    let mut competition = Competition::new(file.header.clone(), 3).unwrap();
    competition.observe(&events, ts(3_000_000_000)).unwrap();
    competition
        .add_replay(&source, file, limits(), OpponentKind::Own, "complete")
        .unwrap();
    let (_, short, _) = recording(99, false);
    competition
        .add_replay(&source, short, limits(), OpponentKind::Other, "prefix")
        .unwrap();
    assert_eq!(competition.opponents[0].score, competition.score);
    assert_eq!(
        (
            competition.opponents[1].score.hits,
            competition.opponents[1].score.misses
        ),
        (1, 0)
    );
    for ghost in &competition.opponents {
        assert_eq!(ghost.song_time, competition.song_time);
    }
    let storage = scratch(&competition);
    let mut reference = OldTransaction::new(&competition);
    reference.update(&[], ts(0), true).unwrap();
    competition.rebuild(&[], ts(0)).unwrap();
    assert_eq!(values(&competition), reference.values);
    assert_eq!(scratch(&competition), storage);
}

#[test]
fn failed_capacity_header_and_record_admission_do_not_consume_members_or_scratch_capacity() {
    let (source, file, _) = recording(1, true);
    let mut competition = Competition::new(file.header.clone(), 1).unwrap();
    let mut wrong = file.clone();
    wrong.header.seed ^= 1;
    assert!(matches!(
        competition.add_replay(&source, wrong, limits(), OpponentKind::Own, "wrong"),
        Err(CompetitionError::IncompatibleSetup)
    ));
    assert_eq!(scratch(&competition).1, 0);
    let mut malformed = file.clone();
    malformed.runtime_version.push_str("-incompatible");
    assert!(
        competition
            .add_replay(&source, malformed, limits(), OpponentKind::Own, "malformed")
            .is_err()
    );
    assert_eq!(scratch(&competition).1, 0);
    let mut invalid_records = file.clone();
    assert_eq!(invalid_records.records.len(), 2);
    invalid_records.records[1].ordinal = invalid_records.records[0].ordinal;
    assert!(
        competition
            .add_replay(
                &source,
                invalid_records,
                limits(),
                OpponentKind::Other,
                "duplicate ordinal"
            )
            .is_err()
    );
    assert_eq!(scratch(&competition).1, 0);
    assert!(competition.opponents.is_empty());
    competition
        .add_replay(&source, file.clone(), limits(), OpponentKind::Own, "valid")
        .unwrap();
    let before = values(&competition);
    let storage = scratch(&competition);
    assert!(matches!(
        competition.add_replay(&source, file, limits(), OpponentKind::Other, "excess"),
        Err(CompetitionError::TooManyOpponents)
    ));
    assert_eq!(values(&competition), before);
    assert_eq!(scratch(&competition), storage);
    assert_eq!(competition.remaining_opponent_capacity(), 0);
}
