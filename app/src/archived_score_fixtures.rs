//! Independent v1/v2 wire and whole-roster historical score fixtures.
use super::*;
use crate::{
    competition::ScoreSummary,
    gauge::BmsGauge,
    timing::{TimingRecord, TimingSummary},
};
use beatkernel::{
    chart::ObjectId,
    judge::{JudgeEvent, JudgeOutcome, JudgeStage},
    time::Duration,
};

fn summary() -> ScoreSummary {
    let mut summary = ScoreSummary::default();
    for (grade, delta) in [(0, -2), (u32::MAX, 3)] {
        summary
            .observe(&[JudgeEvent {
                object: ObjectId(1),
                stage: JudgeStage::Instant,
                outcome: JudgeOutcome::Hit {
                    grade: JudgeGrade(grade),
                    delta: Duration::from_nanos(delta),
                },
                at: Timestamp::ZERO,
                input: None,
            }])
            .unwrap();
    }
    summary
}
fn detailed(score: &ScoreSummary) -> ResultArchive {
    let gauge = BmsGauge::default();
    ResultArchive::from_completed_with_scores(
        &[(
            PlayerId(7),
            CompletedPlayResult::from_completed(Timestamp::ZERO, None, &gauge),
        )],
        &[(
            PlayerId(7),
            super::fixtures::header(0, None),
            gauge.profile().clone(),
        )],
        &[(PlayerId(7), score)],
    )
    .unwrap()
}
fn literal_v2() -> Vec<u8> {
    let mut bytes = super::fixtures::golden();
    bytes[8..12].copy_from_slice(&2u32.to_le_bytes());
    bytes.push(1);
    for counter in [2u64, 0, 2, 2] {
        bytes.extend_from_slice(&counter.to_le_bytes());
    }
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for grade in [0u32, u32::MAX] {
        bytes.extend_from_slice(&grade.to_le_bytes());
        bytes.extend_from_slice(&1u64.to_le_bytes());
    }
    for count in [2u64, 1, 1, 0] {
        bytes.extend_from_slice(&count.to_le_bytes());
    }
    bytes.extend_from_slice(&1i128.to_le_bytes());
    bytes.extend_from_slice(&5u128.to_le_bytes());
    for value in [3i64, -2, 3] {
        bytes.push(1);
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}
#[test]
fn literal_versions_preserve_old_wire_and_independent_v2_field_order() {
    assert_eq!(LEGACY_VERSION, 1);
    assert_eq!(VERSION, 2);
    let legacy = super::fixtures::golden();
    assert_eq!(encode_archive(&super::fixtures::archive()).unwrap(), legacy);
    assert!(
        decode_archive(&legacy).unwrap().entries()[0]
            .score
            .is_none()
    );
    let expected = literal_v2();
    assert_eq!(encode_archive(&detailed(&summary())).unwrap(), expected);
    let row = decode_archive(&expected).unwrap().entries()[0].clone();
    assert_eq!(
        row.score,
        Some(ArchivedScore {
            hits: 2,
            misses: 0,
            combo: 2,
            max_combo: 2,
            grades: vec![(0, 1), (u32::MAX, 1)],
            timing: TimingRecord {
                count: 2,
                early: 1,
                late: 1,
                exact: 0,
                sum: 1,
                absolute_sum: 5,
                last: Some(3),
                min: Some(-2),
                max: Some(3)
            }
        })
    );
    let mut v2_absent = legacy.clone();
    v2_absent[8..12].copy_from_slice(&2u32.to_le_bytes());
    v2_absent.push(0);
    let decoded = decode_archive(&v2_absent).unwrap();
    assert!(decoded.entries()[0].score.is_none());
    assert_eq!(encode_archive(&decoded).unwrap(), legacy);
}
#[test]
fn integer_extremes_have_exact_128_bit_wire_and_no_rounded_mean_fields() {
    let mut score = summary();
    let mut timing = TimingSummary::default();
    for delta in [i64::MIN, i64::MAX, 0] {
        timing
            .observe(&[JudgeEvent {
                object: ObjectId(1),
                stage: JudgeStage::HoldTail,
                outcome: JudgeOutcome::Hit {
                    grade: JudgeGrade(0),
                    delta: Duration::from_nanos(delta),
                },
                at: Timestamp::ZERO,
                input: None,
            }])
            .unwrap();
    }
    // Numeric public summary boundary injection, distinct from real observations.
    score.hits = u64::MAX;
    score.misses = u64::MAX;
    score.combo = u64::MAX;
    score.max_combo = u64::MAX;
    score.grades.clear();
    score.grades.insert(u32::MAX, u64::MAX);
    score.timing = timing;
    let mut archive = detailed(&score);
    let base = super::fixtures::golden().len();
    let bytes = encode_archive(&archive).unwrap();
    assert_eq!(&bytes[base + 1..base + 9], &u64::MAX.to_le_bytes());
    let timing_start = base + 37 + 12;
    assert_eq!(
        &bytes[timing_start + 32..timing_start + 48],
        &(-1i128).to_le_bytes()
    );
    assert_eq!(
        &bytes[timing_start + 48..timing_start + 64],
        &(u64::MAX as u128).to_le_bytes()
    );
    assert_eq!(
        decode_archive(&bytes).unwrap().entries()[0]
            .score
            .as_ref()
            .unwrap()
            .timing,
        timing.record()
    );
    let count = u64::MAX;
    let sum = (count as i128) * (i64::MIN as i128);
    archive.entries[0].score.as_mut().unwrap().timing = TimingRecord {
        count,
        early: count,
        late: 0,
        exact: 0,
        sum,
        absolute_sum: sum.unsigned_abs(),
        last: Some(i64::MIN),
        min: Some(i64::MIN),
        max: Some(i64::MIN),
    };
    let bytes = encode_archive(&archive).unwrap();
    assert_eq!(
        &bytes[timing_start + 32..timing_start + 48],
        &sum.to_le_bytes()
    );
    assert_eq!(
        &bytes[timing_start + 48..timing_start + 64],
        &sum.unsigned_abs().to_le_bytes()
    );
    assert_eq!(decode_archive(&bytes).unwrap(), archive);
}
#[test]
fn whole_and_member_details_associate_by_original_id_with_long_practice_identity() {
    for count in [1u32, 2, 64] {
        let gauge = BmsGauge::default();
        let start = Timestamp::from_nanos(72_000_000_000_000);
        let end = Timestamp::from_nanos(604_800_000_000_000);
        let result = CompletedPlayResult::from_completed(start, Some(end), &gauge);
        let ids: Vec<_> = (0..count)
            .map(|index| PlayerId(u32::MAX - index * 17))
            .collect();
        let rows: Vec<_> = ids.iter().map(|id| (*id, result)).collect();
        let summaries: Vec<_> = ids
            .iter()
            .enumerate()
            .map(|(index, _)| ScoreSummary {
                hits: index as u64 + 1,
                combo: 1,
                max_combo: 1,
                grades: [(u32::MAX - index as u32, index as u64 + 1)]
                    .into_iter()
                    .collect(),
                ..Default::default()
            })
            .collect();
        let mut identities: Vec<_> = ids
            .iter()
            .enumerate()
            .map(|(index, id)| {
                let mut header = super::fixtures::header(start.as_nanos(), Some(end.as_nanos()));
                header.seed = u64::MAX - index as u64;
                (*id, header, gauge.profile().clone())
            })
            .collect();
        identities.reverse();
        let scores: Vec<_> = ids
            .iter()
            .zip(&summaries)
            .rev()
            .map(|(id, score)| (*id, score))
            .collect();
        let archive =
            ResultArchive::from_completed_with_scores(&rows, &identities, &scores).unwrap();
        for (index, row) in archive.entries().iter().enumerate() {
            assert_eq!(row.player, ids[index]);
            assert_eq!(row.header.seed, u64::MAX - index as u64);
            assert_eq!(
                row.score,
                Some(ArchivedScore::from_summary(&summaries[index]).unwrap())
            );
            let member = archive.for_player(ids[index]).unwrap();
            assert_eq!(
                decode_archive(&encode_archive(&member).unwrap())
                    .unwrap()
                    .entries(),
                &[row.clone()]
            );
        }
    }
}
#[test]
fn malformed_v2_positions_truncations_unknown_versions_and_partial_rosters_reject_whole_decode() {
    let valid = literal_v2();
    let base = super::fixtures::golden().len();
    let timing = base + 61;
    for length in 0..valid.len() {
        assert!(
            decode_archive(&valid[..length]).is_err(),
            "truncation {length}"
        );
    }
    for (offset, byte) in [
        (8, 3),
        (base, 2),
        (base + 33, 0),
        (base + 41, 0),
        (timing, 3),
        (timing + 64, 2),
        (timing + 73, 0),
    ] {
        let mut bad = valid.clone();
        bad[offset] = byte;
        assert!(decode_archive(&bad).is_err(), "malformed field {offset}");
    }
    let mut count = valid.clone();
    count[base + 33..base + 37].copy_from_slice(&4097u32.to_le_bytes());
    assert!(decode_archive(&count).is_err());
    let mut duplicate = valid.clone();
    duplicate[base + 49..base + 53].copy_from_slice(&0u32.to_le_bytes());
    assert!(decode_archive(&duplicate).is_err());
    let mut combo = valid.clone();
    combo[base + 17..base + 25].copy_from_slice(&3u64.to_le_bytes());
    assert!(decode_archive(&combo).is_err());
    let mut sum = valid.clone();
    sum[timing + 32..timing + 48].copy_from_slice(&i128::MAX.to_le_bytes());
    assert!(decode_archive(&sum).is_err());
    let mut trailing = valid.clone();
    trailing.push(0);
    assert!(decode_archive(&trailing).is_err());
    let mut partial = valid.clone();
    partial[12..16].copy_from_slice(&2u32.to_le_bytes());
    let second = partial.len();
    partial.extend_from_slice(&super::fixtures::golden()[16..]);
    partial.push(0);
    partial[second..second + 4].copy_from_slice(&9u32.to_le_bytes());
    assert!(decode_archive(&partial).is_err());
}
#[test]
fn score_counter_grade_and_timing_admission_refuses_invalid_public_states() {
    let valid = summary();
    for field in 0..8 {
        let mut bad = valid.clone();
        match field {
            0 => bad.hits = 1,
            1 => bad.combo = 3,
            2 => bad.max_combo = 3,
            3 => {
                bad.grades.insert(7, 0);
            }
            4 => {
                bad.grades.clear();
            }
            5 => bad.timing = TimingSummary::exhausted_for_fixture(),
            6 => {
                bad.grades.insert(7, u64::MAX);
            }
            _ => bad.max_combo = 1,
        }
        assert!(
            ArchivedScore::from_summary(&bad).is_err(),
            "invalid score {field}"
        );
    }
    assert_eq!(MAX_SCORE_GRADES, 4096);
    let at_limit = ScoreSummary {
        hits: 4096,
        grades: (0..4096).map(|grade| (grade, 1)).collect(),
        ..Default::default()
    };
    ArchivedScore::from_summary(&at_limit).unwrap();
    let mut above = at_limit;
    above.hits += 1;
    above.grades.insert(4096, 1);
    assert!(ArchivedScore::from_summary(&above).is_err());
    let mut archive = detailed(&valid);
    archive.entries[0].score.as_mut().unwrap().grades.reverse();
    assert!(encode_archive(&archive).is_err());
}
#[test]
fn score_table_missing_foreign_duplicate_or_bad_later_member_is_atomic_refusal() {
    let gauge = BmsGauge::default();
    let result = CompletedPlayResult::from_completed(Timestamp::ZERO, None, &gauge);
    let rows = [(PlayerId(7), result), (PlayerId(u32::MAX), result)];
    let identities = [
        (
            PlayerId(7),
            super::fixtures::header(0, None),
            gauge.profile().clone(),
        ),
        (
            PlayerId(u32::MAX),
            super::fixtures::header(0, None),
            gauge.profile().clone(),
        ),
    ];
    let valid = summary();
    let mut invalid = valid.clone();
    invalid.hits = 0;
    for scores in [
        vec![],
        vec![(PlayerId(7), &valid)],
        vec![(PlayerId(7), &valid), (PlayerId(7), &valid)],
        vec![(PlayerId(7), &valid), (PlayerId(9), &valid)],
        vec![(PlayerId(7), &valid), (PlayerId(u32::MAX), &invalid)],
    ] {
        assert!(ResultArchive::from_completed_with_scores(&rows, &identities, &scores).is_err());
    }
}

#[cfg(feature = "graphics")]
#[test]
fn associated_historical_details_render_stored_provenance_and_repeat_identical_cached_geometry() {
    use crate::{historical_record_presentation::HistoricalRecordPresentation, scene::Scene};
    let archive = detailed(&summary());
    let replay = encode_replay(
        &ReplayFile::new(archive.entries()[0].header.clone(), vec![]),
        super::replay_limits(),
    )
    .unwrap();
    let bytes = encode_archive(&archive).unwrap();
    let presentation = HistoricalRecordPresentation::new(&replay, Some(&bytes), Some(PlayerId(7)))
        .unwrap()
        .unwrap();
    assert_eq!(presentation.score(), archive.entries()[0].score.as_ref());
    let mut scene = Scene::new(960, 720);
    presentation.compose(&mut scene).unwrap();
    let first: Vec<_> = scene
        .rectangles()
        .iter()
        .map(|rectangle| (rectangle.bounds, rectangle.uv, rectangle.color))
        .collect();
    for label in [
        "STORED HITS 2 MISSES 0",
        "STORED COMBO 2 MAX COMBO 2",
        "TIMING SUM 1 NS",
        "TIMING ABSOLUTE SUM 5 NS",
        "TIMING MIN -2 NS",
    ] {
        let expected: Vec<_> = label.chars().map(crate::font::glyph_uv).collect();
        let actual: Vec<_> = scene
            .rectangles()
            .iter()
            .map(|rectangle| rectangle.uv)
            .collect();
        assert!(
            actual
                .windows(expected.len())
                .any(|window| window == expected.as_slice()),
            "missing stored label {label}"
        );
    }
    assert!(scene.playfields().is_empty());
    scene.clear();
    presentation.compose(&mut scene).unwrap();
    assert_eq!(
        scene
            .rectangles()
            .iter()
            .map(|rectangle| (rectangle.bounds, rectangle.uv, rectangle.color))
            .collect::<Vec<_>>(),
        first
    );
    let legacy = encode_archive(&super::fixtures::archive()).unwrap();
    let legacy = HistoricalRecordPresentation::new(&replay, Some(&legacy), Some(PlayerId(7)))
        .unwrap()
        .unwrap();
    assert!(legacy.score().is_none());
}
