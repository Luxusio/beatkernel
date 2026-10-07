use crate::{
    competition::ScoreSummary,
    judgment_policy::{BmsJudgmentPolicy, BmsScoreSummary, GradeClass, JudgmentPolicyError},
};
use beatkernel_bms::BmsJudgment;
use beatkernel::{
    chart::ObjectId,
    input::codec::CodecLimits,
    judge::{
        JudgeEvent, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeStage, JudgeWindow, MissReason,
    },
    replay::{ReplayHeader, REPLAY_VERSION, codec::ReplayCodecLimits},
    time::{ClockDomainId, Duration, Timestamp},
};
use crate::replay_judgment_policy::{split_options, wrap_header};

fn classes() -> [GradeClass; 4] {
    [
        (u32::MAX, BmsJudgment::PGreat),
        (0, BmsJudgment::Great),
        (77, BmsJudgment::Good),
        (7, BmsJudgment::Bad),
    ]
    .map(|(grade, class)| GradeClass {
        grade: JudgeGrade(grade),
        class,
    })
}
fn profile(grades: &[u32]) -> JudgeProfile {
    JudgeProfile::new(
        grades
            .iter()
            .enumerate()
            .map(|(index, grade)| JudgeWindow {
                grade: JudgeGrade(*grade),
                early: Duration::from_nanos(index as i64 + 1),
                late: Duration::from_nanos(index as i64 + 1),
            })
            .collect(),
        Duration::from_nanos(-91),
    )
    .unwrap()
}
fn limits(header_bytes: usize) -> ReplayCodecLimits {
    ReplayCodecLimits::new(
        65536,
        128,
        header_bytes,
        CodecLimits::new(4096, 1024).unwrap(),
    )
    .unwrap()
}
fn header() -> ReplayHeader {
    ReplayHeader {
        version: REPLAY_VERSION,
        chart_identity: vec![1, 2],
        rules_identity: vec![3],
        options: b"bms-judge-profile/v1:example".to_vec(),
        seed: 0,
        normalized_clock: ClockDomainId(17),
    }
}

#[test]
fn all_hit_classes_project_actual_stages_with_opaque_grades_and_preserve_core_score() {
    let policy = BmsJudgmentPolicy::new(&classes()).unwrap();
    let mut events: Vec<_> = [u32::MAX, u32::MAX, 0, 77, 7]
        .into_iter()
        .enumerate()
        .map(|(i, grade)| JudgeEvent {
            object: ObjectId(i as u64),
            stage: JudgeStage::Instant,
            outcome: JudgeOutcome::Hit {
                grade: JudgeGrade(grade),
                delta: Duration::ZERO,
            },
            at: Timestamp::ZERO,
            input: None,
        })
        .collect();
    events.push(JudgeEvent {
        object: ObjectId(9),
        stage: JudgeStage::HoldTail,
        outcome: JudgeOutcome::Miss {
            reason: MissReason::TailTimeout,
        },
        at: Timestamp::ZERO,
        input: None,
    });
    let mut score = ScoreSummary::default();
    score.observe(&events).unwrap();
    let before = score.clone();
    assert_eq!(
        policy.project(&score),
        Ok(BmsScoreSummary {
            pgreat: 2,
            great: 1,
            good: 1,
            bad: 1,
            poor: 1,
            ex_score: 5,
        })
    );
    assert_eq!(score, before);
    assert_eq!(score.combo, 0);
    assert_eq!(score.max_combo, 5);
    assert_eq!(
        policy.project(&ScoreSummary::default()),
        Ok(BmsScoreSummary::default())
    );
}

#[test]
fn classes_are_bounded_canonical_and_profile_validation_requires_the_exact_grade_set() {
    let policy = BmsJudgmentPolicy::new(&classes()).unwrap();
    assert_eq!(
        policy
            .entries()
            .iter()
            .map(|e| e.grade.0)
            .collect::<Vec<_>>(),
        [0, 7, 77, u32::MAX]
    );
    for entry in classes() {
        assert_eq!(policy.class(entry.grade), Some(entry.class));
    }
    assert_eq!(policy.class(JudgeGrade(1)), None);
    assert_eq!(
        policy.validate_profile(&profile(&[u32::MAX, 0, 77, 7])),
        Ok(())
    );
    for grades in [
        &[u32::MAX, 0, 77][..],
        &[u32::MAX, 0, 77, 8][..],
        &[u32::MAX, 0, 77, 7, 8][..],
    ] {
        assert_eq!(
            policy.validate_profile(&profile(grades)),
            Err(JudgmentPolicyError::ProfileMismatch)
        );
    }
    assert_eq!(
        BmsJudgmentPolicy::new(&[]),
        Err(JudgmentPolicyError::InvalidClasses)
    );
    assert_eq!(
        BmsJudgmentPolicy::new(&[classes()[0]; 2]),
        Err(JudgmentPolicyError::InvalidClasses)
    );
    for class in [BmsJudgment::Poor, BmsJudgment::EmptyPoor] {
        assert_eq!(
            BmsJudgmentPolicy::new(&[GradeClass {
                grade: JudgeGrade(0),
                class
            }]),
            Err(JudgmentPolicyError::InvalidClasses)
        );
    }
    let entries: Vec<_> = (0..65)
        .map(|grade| GradeClass {
            grade: JudgeGrade(grade),
            class: BmsJudgment::Good,
        })
        .collect();
    let max = BmsJudgmentPolicy::new(&entries[..64]).unwrap();
    assert_eq!(max.entries().len(), 64);
    assert_eq!(max.class(JudgeGrade(63)), Some(BmsJudgment::Good));
    assert_eq!(
        BmsJudgmentPolicy::new(&entries),
        Err(JudgmentPolicyError::InvalidClasses)
    );
    assert_eq!(max.clone(), max);
}

#[test]
fn projection_refuses_unknown_inconsistent_and_every_counter_or_ex_overflow() {
    let policy = BmsJudgmentPolicy::new(&classes()).unwrap();
    let score = |hits, misses, grades: &[(u32, u64)]| ScoreSummary {
        hits,
        misses,
        grades: grades.iter().copied().collect(),
        ..Default::default()
    };
    for value in [score(1, 0, &[(42, 1)]), score(0, 0, &[(42, 0)])] {
        assert_eq!(
            policy.project(&value),
            Err(JudgmentPolicyError::UnknownGrade)
        );
    }
    for value in [
        score(2, 0, &[(0, 1)]),
        score(0, 0, &[(0, 1)]),
        score(1, 0, &[]),
    ] {
        assert_eq!(
            policy.project(&value),
            Err(JudgmentPolicyError::InconsistentScore)
        );
    }
    for value in [
        score(u64::MAX, 0, &[(7, u64::MAX), (77, 1)]),
        score(u64::MAX, 1, &[(7, u64::MAX)]),
        score(u64::MAX / 2 + 1, 0, &[(u32::MAX, u64::MAX / 2 + 1)]),
        score(u64::MAX / 2 + 2, 0, &[(u32::MAX, u64::MAX / 2), (0, 2)]),
    ] {
        assert_eq!(policy.project(&value), Err(JudgmentPolicyError::Overflow));
    }
    let repeated = BmsJudgmentPolicy::new(&[
        GradeClass {
            grade: JudgeGrade(7),
            class: BmsJudgment::Bad,
        },
        GradeClass {
            grade: JudgeGrade(77),
            class: BmsJudgment::Bad,
        },
    ])
    .unwrap();
    assert_eq!(
        repeated.project(&score(u64::MAX, 0, &[(7, u64::MAX), (77, 1)])),
        Err(JudgmentPolicyError::Overflow)
    );
    assert_eq!(
        policy
            .project(&score(u64::MAX, 0, &[(0, u64::MAX)]))
            .unwrap()
            .ex_score,
        u64::MAX
    );
    assert_eq!(
        policy.project(&score(0, u64::MAX, &[])).unwrap().poor,
        u64::MAX
    );
}

#[test]
fn class_metadata_is_canonical_bounded_and_never_guesses_legacy_meaning() {
    let policy = BmsJudgmentPolicy::new(&classes()).unwrap();
    let original = header();
    assert_eq!(
        wrap_header(original.clone(), None, limits(8192)).unwrap(),
        original
    );
    assert_eq!(
        split_options(&original.options).unwrap(),
        (original.options.as_slice(), None)
    );
    let wrapped = wrap_header(original.clone(), Some(&policy), limits(8192)).unwrap();
    assert_eq!(
        split_options(&wrapped.options).unwrap(),
        (original.options.as_slice(), Some(policy.clone()))
    );
    let reversed: Vec<_> = classes().into_iter().rev().collect();
    assert_eq!(
        wrap_header(
            original.clone(),
            Some(&BmsJudgmentPolicy::new(&reversed).unwrap()),
            limits(8192)
        )
        .unwrap(),
        wrapped
    );
    let total = wrapped.options.len()
        + wrapped.chart_identity.len()
        + wrapped.rules_identity.len()
        + env!("CARGO_PKG_VERSION").len();
    assert!(wrap_header(original.clone(), Some(&policy), limits(total)).is_ok());
    assert!(wrap_header(original, Some(&policy), limits(total - 1)).is_err());
    assert!(wrap_header(wrapped, Some(&policy), limits(8192)).is_err());
}

#[test]
fn malformed_class_metadata_refuses_truncation_extents_order_tags_and_nesting() {
    const PREFIX: &[u8] = b"bms-judgment-setup/v1:";
    let original = header();
    let policy = BmsJudgmentPolicy::new(&classes()).unwrap();
    let options = wrap_header(original.clone(), Some(&policy), limits(8192))
        .unwrap()
        .options;
    for end in 0..options.len() {
        let result = split_options(&options[..end]);
        if end < PREFIX.len() {
            assert_eq!(result.unwrap(), (&options[..end], None));
        } else {
            assert!(result.is_err(), "truncated extent {end}");
        }
    }
    let count = PREFIX.len() + 4 + original.options.len();
    let first = count + 1;
    for bad_count in [0, 3, 5, 65, 255] {
        let mut changed = options.clone();
        changed[count] = bad_count;
        assert!(split_options(&changed).is_err());
    }
    for length in [0, 1, u32::MAX] {
        let mut changed = options.clone();
        changed[PREFIX.len()..PREFIX.len() + 4].copy_from_slice(&length.to_le_bytes());
        assert!(split_options(&changed).is_err());
    }
    for tag in 4..=255 {
        let mut changed = options.clone();
        changed[first + 4] = tag;
        assert!(split_options(&changed).is_err());
    }
    let mut duplicate = options.clone();
    duplicate[first + 5..first + 9].copy_from_slice(&0u32.to_le_bytes());
    assert!(split_options(&duplicate).is_err());
    let mut reversed = options.clone();
    reversed[first..first + 4].copy_from_slice(&8u32.to_le_bytes());
    assert!(split_options(&reversed).is_err());
    let mut trailing = options.clone();
    trailing.push(0);
    assert!(split_options(&trailing).is_err());
    let mut nested = PREFIX.to_vec();
    nested.extend_from_slice(&(options.len() as u32).to_le_bytes());
    nested.extend_from_slice(&options);
    nested.push(1);
    nested.extend_from_slice(&0u32.to_le_bytes());
    nested.push(0);
    assert!(split_options(&nested).is_err());
}
