//! Deferred independent version-1 archive wire and policy fixtures.
use crate::{
    result_archive::*,
    gauge::{BmsGauge, GaugeProfile, GradeDelta},
    local_players::PlayerId,
    play_result::{CompletedPlayResult, PlayResultScope, PlayResultOutcome},
};
use beatkernel::{
    judge::JudgeGrade,
    replay::ReplayHeader,
    time::{ClockDomainId, Timestamp},
};

pub(crate) fn header(start: i64, end: Option<i64>) -> ReplayHeader {
    let mut options = if end.is_some() {
        b"bms-judge-profile/v4:".to_vec()
    } else if start != 0 {
        b"bms-judge-profile/v2:".to_vec()
    } else {
        b"bms-judge-profile/v1:".to_vec()
    };
    if let Some(end) = end {
        options.extend_from_slice(&9u64.to_le_bytes());
        options.extend_from_slice(&start.to_le_bytes());
        options.extend_from_slice(&end.to_le_bytes());
    } else if start != 0 {
        options.extend_from_slice(&start.to_le_bytes());
    }
    options.extend_from_slice(&0i64.to_le_bytes());
    options.extend_from_slice(&1u64.to_le_bytes());
    options.extend_from_slice(&1u32.to_le_bytes());
    options.extend_from_slice(&0i64.to_le_bytes());
    options.extend_from_slice(&0i64.to_le_bytes());
    ReplayHeader {
        version: 1,
        chart_identity: vec![b'x'],
        rules_identity: vec![b'r'],
        options,
        seed: 9,
        normalized_clock: ClockDomainId(7),
    }
}
pub(crate) fn archive() -> ResultArchive {
    let gauge = BmsGauge::default();
    ResultArchive::from_completed(
        &[(
            PlayerId(7),
            CompletedPlayResult::from_completed(Timestamp::ZERO, None, &gauge),
        )],
        &[(PlayerId(7), header(0, None), gauge.profile().clone())],
    )
    .unwrap()
}
pub(crate) fn invalid_archive() -> ResultArchive {
    let mut entries = archive().entries().to_vec();
    entries[0].player = PlayerId(0);
    ResultArchive { entries }
}
fn literal_replay() -> Vec<u8> {
    let mut out = b"BKREPLAY".to_vec();
    out.extend_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0, 7, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0]);
    // Four canonical u64-sized blobs; no production encoder supplies expected bytes.
    let mut options = b"bms-judge-profile/v1:".to_vec();
    options.extend_from_slice(&[0; 8]);
    options.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0]);
    options.extend_from_slice(&[1, 0, 0, 0]);
    options.extend_from_slice(&[0; 16]);
    for field in [&b"0.1.0"[..], &b"x"[..], &b"r"[..], options.as_slice()] {
        out.extend_from_slice(&(field.len() as u64).to_le_bytes());
        out.extend_from_slice(field);
    }
    out.push(0);
    out.extend_from_slice(&[0; 8]);
    out
}
pub(crate) fn golden() -> Vec<u8> {
    let replay = literal_replay();
    let mut out = b"BKRESULT".to_vec();
    out.extend_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0, 7, 0, 0, 0]);
    out.extend_from_slice(&(replay.len() as u32).to_le_bytes());
    out.extend_from_slice(&replay);
    out.extend_from_slice(&20_000_000u64.to_le_bytes());
    out.extend_from_slice(&80_000_000u64.to_le_bytes());
    out.extend_from_slice(&1_000_000i64.to_le_bytes());
    out.extend_from_slice(&(-6_000_000i64).to_le_bytes());
    out.push(0);
    out.extend_from_slice(&[0; 4]); // fail-on-empty, override count.
    out.push(0); // FullSong, with no start/end payload.
    out.extend_from_slice(&20_000_000u64.to_le_bytes());
    out.extend_from_slice(&[0, 1]); // healthy, below threshold.
    out
}
#[test]
fn literal_header_profile_scope_and_classification_match_version_one_wire() {
    let expected = golden();
    assert_eq!(encode_archive(&archive()).unwrap(), expected);
    let historical = decode_archive(&expected).unwrap();
    let row = &historical.entries()[0];
    assert_eq!(row.player, PlayerId(7));
    assert_eq!(row.header, header(0, None));
    assert_eq!(row.profile, GaugeProfile::default());
    assert_eq!(row.result.scope, PlayResultScope::FullSong);
    assert_eq!(row.result.outcome, PlayResultOutcome::BelowClearThreshold);
    assert_eq!(row.result.gauge.level_units, 20_000_000);
    assert_eq!(row.result.gauge.failure, None);
}
#[test]
fn every_truncation_and_adversarial_later_row_reject_the_whole_archive() {
    let bytes = golden();
    for length in 0..bytes.len() {
        assert!(
            decode_archive(&bytes[..length]).is_err(),
            "accepted truncation {length}"
        );
    }
    let profile = 24 + literal_replay().len();
    for (offset, replacement) in [
        (0, 0),
        (8, 2),
        (12, 0),
        (12, 65),
        (16, 0),
        (profile + 32, 2),
        (profile + 37, 2),
        (bytes.len() - 2, 9),
        (bytes.len() - 1, 9),
    ] {
        let mut bad = bytes.clone();
        bad[offset] = replacement;
        assert!(
            decode_archive(&bad).is_err(),
            "accepted corrupted field {offset}"
        );
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(decode_archive(&trailing).is_err());
    let mut oversized_length = bytes.clone();
    oversized_length[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode_archive(&oversized_length).is_err());
    // Complete second row, then corrupt only its identity: prefix success cannot leak a partial table.
    let mut two = bytes.clone();
    two[12..16].copy_from_slice(&2u32.to_le_bytes());
    two.extend_from_slice(&bytes[16..]);
    assert!(
        decode_archive(&two).is_err(),
        "duplicate later player accepted"
    );
    let second = bytes.len();
    two[second..second + 4].copy_from_slice(&8u32.to_le_bytes());
    assert_eq!(decode_archive(&two).unwrap().entries().len(), 2);
    let last = two.len() - 1;
    two[last] = 0; // A healthy 20% row cannot claim 80% clear.
    assert!(decode_archive(&two).is_err());
    let mut too_large = vec![0; 5 * 1024 * 1024 + 1];
    too_large[..8].copy_from_slice(b"BKRESULT");
    assert!(decode_archive(&too_large).is_err());
}
#[test]
fn every_supported_roster_preserves_nonconsecutive_original_ids_and_each_header() {
    for count in 1..=64 {
        let gauge = BmsGauge::default();
        let result = CompletedPlayResult::from_completed(Timestamp::ZERO, None, &gauge);
        let rows = (0..count)
            .map(|index| (PlayerId(u32::MAX - index * 17), result))
            .collect::<Vec<_>>();
        let identities = rows
            .iter()
            .enumerate()
            .map(|(index, (id, _))| {
                let mut header = header(0, None);
                header.seed = u64::MAX - index as u64;
                header.chart_identity = vec![index as u8, 0xff];
                header.normalized_clock = ClockDomainId(u32::MAX - index as u32);
                (*id, header, gauge.profile().clone())
            })
            .collect::<Vec<_>>();
        let archive = ResultArchive::from_completed(&rows, &identities).unwrap();
        let decoded = decode_archive(&encode_archive(&archive).unwrap()).unwrap();
        assert_eq!(
            decoded
                .entries()
                .iter()
                .map(|row| row.player)
                .collect::<Vec<_>>(),
            rows.iter().map(|row| row.0).collect::<Vec<_>>()
        );
        for (entry, identity) in decoded.entries().iter().zip(&identities) {
            assert_eq!(entry.header, identity.1);
        }
    }
    let gauge = BmsGauge::default();
    let result = CompletedPlayResult::from_completed(Timestamp::ZERO, None, &gauge);
    for rows in [
        vec![],
        vec![(PlayerId(0), result)],
        vec![(PlayerId(1), result), (PlayerId(1), result)],
        (1..=65).map(|id| (PlayerId(id), result)).collect(),
    ] {
        let identities = rows
            .iter()
            .map(|(id, _)| (*id, header(0, None), gauge.profile().clone()))
            .collect::<Vec<_>>();
        assert!(ResultArchive::from_completed(&rows, &identities).is_err());
    }
}
#[test]
fn custom_thresholds_sorted_signed_overrides_and_long_practice_extent_remain_exact() {
    let profile = GaugeProfile::new(
        5,
        4,
        i64::MAX,
        i64::MIN,
        false,
        vec![
            GradeDelta {
                grade: JudgeGrade(u32::MAX),
                delta: i64::MIN,
            },
            GradeDelta {
                grade: JudgeGrade(0),
                delta: i64::MAX,
            },
        ],
    )
    .unwrap();
    let gauge = BmsGauge::new(profile.clone());
    let start = 72_000_000_000_000i64;
    let end = 604_800_000_000_000i64;
    let result = CompletedPlayResult::from_completed(
        Timestamp::from_nanos(start),
        Some(Timestamp::from_nanos(end)),
        &gauge,
    );
    let mut identity = header(start, Some(end));
    identity.seed = u64::MAX;
    let archive = ResultArchive::from_completed(
        &[(PlayerId(u32::MAX), result)],
        &[(PlayerId(u32::MAX), identity.clone(), profile.clone())],
    )
    .unwrap();
    let encoded = encode_archive(&archive).unwrap();
    let profile_offset = 24 + u32::from_le_bytes(encoded[20..24].try_into().unwrap()) as usize;
    let mut expected = Vec::new();
    expected.extend_from_slice(&5u64.to_le_bytes());
    expected.extend_from_slice(&4u64.to_le_bytes());
    expected.extend_from_slice(&i64::MAX.to_le_bytes());
    expected.extend_from_slice(&i64::MIN.to_le_bytes());
    expected.extend_from_slice(&[0, 2, 0, 0, 0]);
    expected.extend_from_slice(&0u32.to_le_bytes());
    expected.extend_from_slice(&i64::MAX.to_le_bytes());
    expected.extend_from_slice(&u32::MAX.to_le_bytes());
    expected.extend_from_slice(&i64::MIN.to_le_bytes());
    assert_eq!(&encoded[profile_offset..profile_offset + 61], expected);
    let decoded = decode_archive(&encoded).unwrap();
    let row = &decoded.entries()[0];
    assert_eq!(row.profile, profile);
    assert_eq!(row.header, identity);
    assert_eq!(row.result.outcome, PlayResultOutcome::Cleared);
    assert_eq!(row.result.gauge.level_units, 5);
    assert_eq!(
        row.result.scope,
        PlayResultScope::PracticeSection {
            start: Timestamp::from_nanos(start),
            end: Some(Timestamp::from_nanos(end))
        }
    );
    let wrong_threshold = GaugeProfile::new(5, 6, 0, 0, false, vec![]).unwrap();
    assert!(
        ResultArchive::from_completed(
            &[(PlayerId(u32::MAX), result)],
            &[(
                PlayerId(u32::MAX),
                header(start, Some(end)),
                wrong_threshold
            )]
        )
        .is_err()
    );
    assert!(
        ResultArchive::from_completed(
            &[(PlayerId(u32::MAX), result)],
            &[(PlayerId(u32::MAX), header(0, None), profile)]
        )
        .is_err()
    );
}
#[test]
fn missing_foreign_duplicate_identity_mixed_scope_and_noncanonical_grades_are_atomic_errors() {
    let gauge = BmsGauge::default();
    let full = CompletedPlayResult::from_completed(Timestamp::ZERO, None, &gauge);
    let practice = CompletedPlayResult::from_completed(
        Timestamp::ZERO,
        Some(Timestamp::from_nanos(1)),
        &gauge,
    );
    let rows = [(PlayerId(7), full), (PlayerId(9), full)];
    for identities in [
        vec![(PlayerId(7), header(0, None), gauge.profile().clone())],
        vec![
            (PlayerId(7), header(0, None), gauge.profile().clone()),
            (PlayerId(7), header(0, None), gauge.profile().clone()),
        ],
        vec![
            (PlayerId(7), header(0, None), gauge.profile().clone()),
            (PlayerId(8), header(0, None), gauge.profile().clone()),
        ],
    ] {
        assert!(ResultArchive::from_completed(&rows, &identities).is_err());
    }
    assert!(
        ResultArchive::from_completed(
            &[(PlayerId(7), full), (PlayerId(9), practice)],
            &[
                (PlayerId(7), header(0, None), gauge.profile().clone()),
                (PlayerId(9), header(0, Some(1)), gauge.profile().clone())
            ]
        )
        .is_err()
    );
    let mut unsupported = header(0, None);
    unsupported.version = 2;
    assert!(
        ResultArchive::from_completed(
            &[(PlayerId(7), full)],
            &[(PlayerId(7), unsupported, gauge.profile().clone())]
        )
        .is_err()
    );
    let mut excessive = header(0, None);
    excessive.chart_identity = vec![0; 65_537];
    assert!(
        ResultArchive::from_completed(
            &[(PlayerId(7), full)],
            &[(PlayerId(7), excessive, gauge.profile().clone())]
        )
        .is_err()
    );
    let bytes = golden();
    let profile = 24 + literal_replay().len();
    for grades in [[9u32, 9u32], [9u32, 1u32]] {
        let mut bad = bytes.clone();
        bad[profile + 33..profile + 37].copy_from_slice(&2u32.to_le_bytes());
        let mut overrides = Vec::new();
        for grade in grades {
            overrides.extend_from_slice(&grade.to_le_bytes());
            overrides.extend_from_slice(&1i64.to_le_bytes());
        }
        bad.splice(profile + 37..profile + 37, overrides);
        assert!(decode_archive(&bad).is_err());
    }
}
#[test]
fn gauge_failure_and_embedded_header_only_policy_reject_inconsistent_history() {
    let bytes = golden();
    let profile = 24 + literal_replay().len();
    for offset in [profile, profile + 8, bytes.len() - 10] {
        let mut bad = bytes.clone();
        bad[offset..offset + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(decode_archive(&bad).is_err());
    }
    for (failure, outcome) in [(1, 2), (2, 3), (0, 2), (0, 3)] {
        let mut bad = bytes.clone();
        let last = bad.len() - 1;
        bad[last - 1] = failure;
        bad[last] = outcome;
        assert!(decode_archive(&bad).is_err());
    }
    let mut death = bytes.clone();
    let end = death.len();
    death[end - 10..end - 2].copy_from_slice(&0u64.to_le_bytes());
    death[end - 2] = 1;
    death[end - 1] = 2;
    assert_eq!(
        decode_archive(&death).unwrap().entries()[0].result.outcome,
        PlayResultOutcome::Failed(crate::gauge::GaugeFailure::InstantDeath)
    );
    death[end - 2] = 2;
    death[end - 1] = 3;
    assert!(
        decode_archive(&death).is_err(),
        "depletion without fail-on-empty accepted"
    );
    death[profile + 32] = 1;
    assert_eq!(
        decode_archive(&death).unwrap().entries()[0].result.outcome,
        PlayResultOutcome::Failed(crate::gauge::GaugeFailure::Depleted)
    );
    death[end - 2] = 0;
    death[end - 1] = 1;
    assert!(
        decode_archive(&death).is_err(),
        "healthy empty failing profile accepted"
    );
    let mut bad_count = bytes.clone();
    bad_count[profile + 33..profile + 37].copy_from_slice(&65u32.to_le_bytes());
    assert!(decode_archive(&bad_count).is_err());
    let mut unsupported = bytes.clone();
    unsupported[36..40].copy_from_slice(&2u32.to_le_bytes());
    assert!(decode_archive(&unsupported).is_err());
    // A fully valid canonical ReplayFile with an Advance is still forbidden in this header-only envelope.
    let mut replay = literal_replay();
    let count = replay.len() - 8;
    replay[count..].copy_from_slice(&1u64.to_le_bytes());
    replay.extend_from_slice(&[0; 16]);
    replay.push(1);
    let mut record_archive = bytes[..20].to_vec();
    record_archive.extend_from_slice(&(replay.len() as u32).to_le_bytes());
    record_archive.extend_from_slice(&replay);
    record_archive.extend_from_slice(&bytes[profile..]);
    assert!(decode_archive(&record_archive).is_err());
    let mut replay = literal_replay();
    let flag = replay.len() - 9;
    replay[flag] = 1;
    replay.splice(flag + 1..flag + 1, [0; 8]); // Some(empty calibration) is also disallowed.
    let mut calibrated = bytes[..20].to_vec();
    calibrated.extend_from_slice(&(replay.len() as u32).to_le_bytes());
    calibrated.extend_from_slice(&replay);
    calibrated.extend_from_slice(&bytes[profile..]);
    assert!(decode_archive(&calibrated).is_err());
}
