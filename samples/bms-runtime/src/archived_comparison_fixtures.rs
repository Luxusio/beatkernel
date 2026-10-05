//! Deferred v3 comparison wire, bounded admission and original-roster projection.
use super::*;
use crate::{
    competition::OpponentKind,
    competition_presentation::{
        CompetitionSnapshot, GhostSnapshot, NetworkSnapshot, NetworkStatus,
    },
    multiplayer::Progress,
};
fn snapshot() -> CompetitionSnapshot {
    CompetitionSnapshot {
        ghosts: vec![
            GhostSnapshot {
                kind: OpponentKind::Own,
                label: "own.bkr".into(),
                hits: u64::MAX,
                misses: 0,
                combo: u64::MAX,
                max_combo: u64::MAX,
                recorded_until: Some(Timestamp::from_nanos(i64::MIN)),
            },
            GhostSnapshot {
                kind: OpponentKind::Other,
                label: "peer.bkr".into(),
                hits: 0,
                misses: u64::MAX,
                combo: 0,
                max_combo: 0,
                recorded_until: None,
            },
        ],
        network: Some(NetworkSnapshot {
            status: NetworkStatus::Stopped,
            progress: Some(Progress {
                song_ns: i64::MAX,
                hits: u64::MAX,
                misses: 0,
                combo: u64::MAX,
                max_combo: u64::MAX,
            }),
        }),
    }
}
struct Literal {
    bytes: Vec<u8>,
    extension: usize,
    tags: Vec<usize>,
    label_len: usize,
    label: usize,
    counters: usize,
    progress: usize,
}
fn literal() -> Literal {
    let mut bytes = super::fixtures::golden();
    bytes[8..12].copy_from_slice(&3u32.to_le_bytes());
    bytes.push(0); // v2 score tag, unavailable
    let extension = bytes.len();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&7u32.to_le_bytes());
    let mut tags = vec![bytes.len()];
    bytes.push(1);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    tags.push(bytes.len());
    bytes.push(0);
    let label_len = bytes.len();
    bytes.extend_from_slice(&7u32.to_le_bytes());
    let label = bytes.len();
    bytes.extend_from_slice(b"own.bkr");
    let counters = bytes.len();
    for count in [u64::MAX, 0, u64::MAX, u64::MAX] {
        bytes.extend_from_slice(&count.to_le_bytes());
    }
    tags.push(bytes.len());
    bytes.push(1);
    bytes.extend_from_slice(&i64::MIN.to_le_bytes());
    tags.push(bytes.len());
    bytes.push(1);
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes.extend_from_slice(b"peer.bkr");
    for count in [0u64, u64::MAX, 0, 0] {
        bytes.extend_from_slice(&count.to_le_bytes());
    }
    tags.push(bytes.len());
    bytes.push(0);
    tags.push(bytes.len());
    bytes.push(1);
    tags.push(bytes.len());
    bytes.push(3);
    tags.push(bytes.len());
    bytes.push(1);
    let progress = bytes.len();
    bytes.extend_from_slice(&i64::MAX.to_le_bytes());
    for count in [u64::MAX, 0, u64::MAX, u64::MAX] {
        bytes.extend_from_slice(&count.to_le_bytes());
    }
    Literal {
        bytes,
        extension,
        tags,
        label_len,
        label,
        counters,
        progress,
    }
}
#[test]
fn independent_literal_v3_keeps_exact_prefix_counters_tags_and_signed_integer_fields() {
    assert_eq!(COMPARISON_VERSION, 3);
    let snapshot = snapshot();
    let mut archive = super::fixtures::archive();
    archive
        .attach_comparisons(&[(PlayerId(7), Some(&snapshot))])
        .unwrap();
    let expected = literal();
    assert_eq!(encode_archive(&archive).unwrap(), expected.bytes);
    let decoded = decode_archive(&expected.bytes).unwrap();
    assert_eq!(
        decoded.comparisons(),
        Some(&[(PlayerId(7), Some(snapshot.clone()))][..])
    );
    assert_eq!(decoded.comparison(PlayerId(7)), Some(&snapshot));
    assert_eq!(decoded.entries()[0].score, None);
}
#[test]
fn legacy_v1_v2_absence_differs_from_explicit_none_and_empty_snapshot_without_changing_old_bytes() {
    let gauge = crate::gauge::BmsGauge::default();
    let result = CompletedPlayResult::from_completed(Timestamp::ZERO, None, &gauge);
    let score = crate::competition::ScoreSummary::default();
    let scored = ResultArchive::from_completed_with_scores(
        &[(PlayerId(7), result)],
        &[(
            PlayerId(7),
            super::fixtures::header(0, None),
            gauge.profile().clone(),
        )],
        &[(PlayerId(7), &score)],
    )
    .unwrap();
    for mut archive in [super::fixtures::archive(), scored] {
        let old = encode_archive(&archive).unwrap();
        assert!(matches!(
            u32::from_le_bytes(old[8..12].try_into().unwrap()),
            1 | 2
        ));
        assert!(decode_archive(&old).unwrap().comparisons().is_none());
        archive.attach_comparisons(&[(PlayerId(7), None)]).unwrap();
        assert_eq!(archive.comparisons(), Some(&[(PlayerId(7), None)][..]));
        assert!(archive.comparison(PlayerId(7)).is_none());
        assert_eq!(
            &encode_archive(&archive).unwrap()[8..12],
            &3u32.to_le_bytes()
        );
        let empty = CompetitionSnapshot {
            ghosts: vec![],
            network: None,
        };
        archive
            .attach_comparisons(&[(PlayerId(7), Some(&empty))])
            .unwrap();
        assert_eq!(archive.comparison(PlayerId(7)), Some(&empty));
    }
    assert_eq!(
        encode_archive(&super::fixtures::archive()).unwrap(),
        super::fixtures::golden()
    );
}
#[test]
fn shuffled_whole_comparison_rows_and_member_projection_retain_all_original_ids_and_headers() {
    for count in [1u32, 2, 64] {
        let mut archive = super::member_fixtures::whole(count);
        let ids: Vec<_> = archive.entries().iter().map(|entry| entry.player).collect();
        let snapshots: Vec<_> = ids
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let mut snapshot = snapshot();
                snapshot.ghosts[0].label = format!("own{index}.bkr");
                snapshot
            })
            .collect();
        let incoming: Vec<_> = ids
            .iter()
            .zip(&snapshots)
            .rev()
            .map(|(id, snapshot)| (*id, Some(snapshot)))
            .collect();
        archive.attach_comparisons(&incoming).unwrap();
        assert_eq!(
            archive
                .comparisons()
                .unwrap()
                .iter()
                .map(|row| row.0)
                .collect::<Vec<_>>(),
            ids
        );
        for (index, id) in ids.iter().enumerate() {
            let member = archive.for_player(*id).unwrap();
            let decoded = decode_archive(&encode_archive(&member).unwrap()).unwrap();
            assert_eq!(decoded.entries(), &[archive.entries()[index].clone()]);
            assert_eq!(
                decoded.comparisons(),
                Some(&[(*id, Some(snapshots[index].clone()))][..])
            );
        }
    }
}
#[test]
fn attachment_is_atomic_on_missing_foreign_duplicate_or_invalid_later_rows_and_normalizes_basename()
{
    let mut archive = super::member_fixtures::whole(2);
    let ids = [archive.entries()[0].player, archive.entries()[1].player];
    let valid = snapshot();
    archive
        .attach_comparisons(&[(ids[0], Some(&valid)), (ids[1], None)])
        .unwrap();
    let before = archive.clone();
    let mut bad = valid.clone();
    bad.ghosts[1].combo = 1;
    for incoming in [
        vec![],
        vec![(ids[0], Some(&valid))],
        vec![(ids[0], None), (ids[0], None)],
        vec![(ids[0], None), (PlayerId(0), None)],
        vec![(ids[0], None), (PlayerId(9), None)],
        vec![(ids[0], None), (ids[1], Some(&bad))],
    ] {
        assert!(archive.attach_comparisons(&incoming).is_err());
        assert_eq!(archive, before);
    }
    let mut path = valid;
    path.ghosts[0].label = "portable\\folder/own.bkr".into();
    archive
        .attach_comparisons(&[(ids[1], None), (ids[0], Some(&path))])
        .unwrap();
    assert_eq!(
        archive.comparison(ids[0]).unwrap().ghosts[0].label,
        "own.bkr"
    );
    assert_eq!(path.ghosts[0].label, "portable\\folder/own.bkr");
    let mut invalid_whole = before;
    invalid_whole.entries[1].player = PlayerId(0);
    assert!(invalid_whole.for_player(ids[0]).is_err());
}
#[test]
fn ghost_label_count_and_peer_counter_bounds_refuse_without_replacing_accepted_snapshot() {
    let mut archive = super::fixtures::archive();
    let valid = snapshot();
    archive
        .attach_comparisons(&[(PlayerId(7), Some(&valid))])
        .unwrap();
    let before = archive.clone();
    for label in [
        "".into(),
        "folder/".into(),
        "a".repeat(65),
        "😀".repeat(65),
        "bad\nlabel".into(),
        "bad\0label".into(),
    ] {
        let mut bad = valid.clone();
        bad.ghosts[0].label = label;
        assert!(
            archive
                .attach_comparisons(&[(PlayerId(7), Some(&bad))])
                .is_err()
        );
        assert_eq!(archive, before);
    }
    let mut eight = valid.clone();
    eight.ghosts = vec![valid.ghosts[0].clone(); 8];
    eight.ghosts[0].label = "😀".repeat(64);
    archive
        .attach_comparisons(&[(PlayerId(7), Some(&eight))])
        .unwrap();
    let at_limit = archive.clone();
    let mut nine = eight;
    nine.ghosts.push(valid.ghosts[0].clone());
    assert!(
        archive
            .attach_comparisons(&[(PlayerId(7), Some(&nine))])
            .is_err()
    );
    assert_eq!(archive, at_limit);
    for field in 0..3 {
        let mut bad = valid.clone();
        let progress = bad.network.as_mut().unwrap().progress.as_mut().unwrap();
        match field {
            0 => progress.hits = 0,
            1 => progress.misses = 1,
            _ => {
                progress.combo = 2;
                progress.max_combo = 1;
            }
        }
        assert!(
            archive
                .attach_comparisons(&[(PlayerId(7), Some(&bad))])
                .is_err()
        );
        assert_eq!(archive, at_limit);
    }
    let mut waiting = valid;
    waiting.network.as_mut().unwrap().status = NetworkStatus::Waiting;
    assert!(
        archive
            .attach_comparisons(&[(PlayerId(7), Some(&waiting))])
            .is_err()
    );
    assert_eq!(archive, at_limit);
    for label in [".", "..", "\u{2028}"] {
        let mut allowed = snapshot();
        allowed.ghosts[0].label = label.into();
        archive
            .attach_comparisons(&[(PlayerId(7), Some(&allowed))])
            .unwrap();
        assert_eq!(
            decode_archive(&encode_archive(&archive).unwrap())
                .unwrap()
                .comparison(PlayerId(7))
                .unwrap()
                .ghosts[0]
                .label,
            label
        );
    }
}
#[test]
fn decoder_rejects_all_truncations_bad_tags_utf8_paths_counts_later_rows_and_trailing_data() {
    let wire = literal();
    for length in 0..wire.bytes.len() {
        assert!(
            decode_archive(&wire.bytes[..length]).is_err(),
            "truncation {length}"
        );
    }
    for offset in &wire.tags {
        let mut bad = wire.bytes.clone();
        bad[*offset] = 9;
        assert!(decode_archive(&bad).is_err(), "tag {offset}");
    }
    for bytes in [vec![0xff], vec![b'\n'], vec![b'/'], vec![b'\\']] {
        let mut bad = wire.bytes.clone();
        bad[wire.label] = bytes[0];
        assert!(decode_archive(&bad).is_err());
    }
    let mut bad = wire.bytes.clone();
    bad[wire.label_len..wire.label_len + 4].copy_from_slice(&257u32.to_le_bytes());
    assert!(decode_archive(&bad).is_err());
    let mut bad = wire.bytes.clone();
    bad[wire.extension + 9..wire.extension + 13].copy_from_slice(&9u32.to_le_bytes());
    assert!(decode_archive(&bad).is_err());
    let mut bad = wire.bytes.clone();
    bad[wire.counters..wire.counters + 8].copy_from_slice(&0u64.to_le_bytes());
    assert!(decode_archive(&bad).is_err());
    let mut bad = wire.bytes.clone();
    bad[wire.progress + 16..wire.progress + 24].copy_from_slice(&1u64.to_le_bytes());
    assert!(decode_archive(&bad).is_err());
    let mut bad = wire.bytes.clone();
    bad[wire.extension + 4..wire.extension + 8].copy_from_slice(&0u32.to_le_bytes());
    assert!(decode_archive(&bad).is_err());
    let mut bad = wire.bytes.clone();
    bad[wire.extension..wire.extension + 4].copy_from_slice(&2u32.to_le_bytes());
    assert!(decode_archive(&bad).is_err());
    let mut bad = wire.bytes.clone();
    bad[wire.tags[6]] = 0;
    assert!(decode_archive(&bad).is_err()); // Waiting cannot carry progress.
    let mut trailing = wire.bytes;
    trailing.push(0);
    assert!(decode_archive(&trailing).is_err());
    assert!(decode_archive(&vec![0; MAX_ARCHIVE_BYTES + 1]).is_err());
}

#[test]
fn individually_valid_large_headers_scores_and_snapshots_refuse_combined_envelope_atomically() {
    let gauge = crate::gauge::BmsGauge::default();
    let result = CompletedPlayResult::from_completed(Timestamp::ZERO, None, &gauge);
    let rows: Vec<_> = (0..64)
        .map(|index| (PlayerId(u32::MAX - index * 17), result))
        .collect();
    let mut header = super::fixtures::header(0, None);
    header.chart_identity = vec![b'x'; 64_300];
    let identities: Vec<_> = rows
        .iter()
        .map(|row| (row.0, header.clone(), gauge.profile().clone()))
        .collect();
    let score = crate::competition::ScoreSummary {
        hits: 1350,
        combo: 1350,
        max_combo: 1350,
        grades: (0..1350).map(|grade| (grade, 1)).collect(),
        ..Default::default()
    };
    let scores: Vec<_> = rows.iter().map(|row| (row.0, &score)).collect();
    let mut archive =
        ResultArchive::from_completed_with_scores(&rows, &identities, &scores).unwrap();
    let base = encode_archive(&archive).unwrap();
    assert!(base.len() <= MAX_ARCHIVE_BYTES);
    let mut selected = snapshot();
    selected.ghosts = vec![selected.ghosts[0].clone(); 8];
    for ghost in &mut selected.ghosts {
        ghost.label = "😀".repeat(64);
    }
    let one = super::fixtures::archive();
    let mut bounded = one.clone();
    bounded
        .attach_comparisons(&[(PlayerId(7), Some(&selected))])
        .unwrap();
    assert!(encode_archive(&bounded).unwrap().len() <= MAX_ARCHIVE_BYTES);
    let comparison_rows: Vec<_> = rows.iter().map(|row| (row.0, Some(&selected))).collect();
    let before = archive.clone();
    assert!(archive.attach_comparisons(&comparison_rows).is_err());
    assert_eq!(archive, before);
    assert!(archive.comparisons().is_none());
    assert_eq!(encode_archive(&archive).unwrap(), base);
}
