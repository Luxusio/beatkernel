//! Deferred byte decoding and cached historical geometry; no GPU or gameplay owner.
use super::*;
use crate::{
    local_players::PlayerId,
    result_archive::{encode_archive, decode_archive},
    play_result::{PlayResultScope, PlayResultOutcome},
};
use beatkernel::{
    replay::codec::{encode_replay, ReplayFile},
    time::Timestamp,
};
fn has_label(scene: &crate::scene::Scene, label: &str) -> bool {
    let expected = label.chars().map(crate::font::glyph_uv).collect::<Vec<_>>();
    let actual = scene
        .rectangles()
        .iter()
        .map(|rectangle| rectangle.uv)
        .collect::<Vec<_>>();
    actual
        .windows(expected.len())
        .any(|window| window == expected.as_slice())
}
fn bytes(start: i64, end: Option<i64>, players: &[PlayerId]) -> (Vec<u8>, Vec<u8>) {
    let capture = crate::native_section_capture_fixtures::capture(start, end, u64::MAX);
    let file: ReplayFile = capture.into_file();
    let archive = crate::record_association_fixtures::archive(
        &players
            .iter()
            .map(|id| (*id, file.header.clone()))
            .collect::<Vec<_>>(),
        start,
        end,
    );
    (
        encode_replay(&file, crate::competition_live::replay_limits().unwrap()).unwrap(),
        encode_archive(&archive).unwrap(),
    )
}
#[test]
fn real_replay_archive_bytes_preserve_original_id_full_or_long_practice_and_no_scores() {
    for (start, end) in [(0, None), (72_000_000_000_000, Some(604_800_000_000_000))] {
        let (replay, archive) = bytes(start, end, &[PlayerId(u32::MAX)]);
        let presentation =
            HistoricalRecordPresentation::new(&replay, Some(&archive), Some(PlayerId(u32::MAX)))
                .unwrap()
                .unwrap();
        let (player, result) = presentation.value();
        assert_eq!(player, PlayerId(u32::MAX));
        assert_eq!(result.gauge.level_units, 20_000_000);
        assert_eq!(result.outcome, PlayResultOutcome::BelowClearThreshold);
        assert_eq!(
            result.scope,
            match end {
                None => PlayResultScope::FullSong,
                Some(end) => PlayResultScope::PracticeSection {
                    start: Timestamp::from_nanos(start),
                    end: Some(Timestamp::from_nanos(end))
                },
            }
        );
        assert_eq!(presentation.start(), Timestamp::from_nanos(start));
        assert_eq!(presentation.end(), end.map(Timestamp::from_nanos));
        let mut scene = crate::scene::Scene::new(960, 720);
        presentation.compose(&mut scene).unwrap();
        assert!(scene.playfields().is_empty());
        assert!(has_label(&scene, "STORED HISTORICAL RECORD"));
        assert!(has_label(&scene, "HISTORICAL PLAYER 4294967295"));
        assert!(has_label(&scene, "STORED GAUGE 20000000 UNITS"));
        assert!(has_label(&scene, &format!("START {start} NS")));
        assert!(!has_label(&scene, "HITS"));
        assert!(!has_label(&scene, "MISSES"));
    }
}
#[test]
fn equal_header_rows_need_explicit_id_and_complete_later_row_decode_before_admission() {
    let (replay, archive) = bytes(0, None, &[PlayerId(7), PlayerId(u32::MAX)]);
    assert!(HistoricalRecordPresentation::new(&replay, Some(&archive), None).is_err());
    assert_eq!(
        HistoricalRecordPresentation::new(&replay, Some(&archive), Some(PlayerId(u32::MAX)))
            .unwrap()
            .unwrap()
            .value()
            .0,
        PlayerId(u32::MAX)
    );
    for id in [PlayerId(0), PlayerId(8)] {
        assert!(HistoricalRecordPresentation::new(&replay, Some(&archive), Some(id)).is_err());
    }
    let mut bad = archive.clone();
    *bad.last_mut().unwrap() = 9;
    assert!(decode_archive(&bad).is_err());
    assert!(HistoricalRecordPresentation::new(&replay, Some(&bad), Some(PlayerId(7))).is_err());
    assert!(
        HistoricalRecordPresentation::new(
            &replay,
            Some(&archive[..archive.len() - 1]),
            Some(PlayerId(7))
        )
        .is_err()
    );
    let other = crate::native_section_capture_fixtures::capture(0, None, 3).into_file();
    let other = encode_replay(&other, crate::competition_live::replay_limits().unwrap()).unwrap();
    assert!(HistoricalRecordPresentation::new(&other, Some(&archive), Some(PlayerId(7))).is_err());
    assert!(
        HistoricalRecordPresentation::new(&replay, None, None)
            .unwrap()
            .is_none()
    );
}
#[test]
fn repeated_composition_reuses_frozen_geometry_without_note_or_prefix_score_projection() {
    let (replay, archive) = bytes(0, Some(72_000_000_000_000), &[PlayerId(99)]);
    let presentation =
        HistoricalRecordPresentation::new(&replay, Some(&archive), Some(PlayerId(99)))
            .unwrap()
            .unwrap();
    let mut scene = crate::scene::Scene::new(960, 720);
    presentation.compose(&mut scene).unwrap();
    let first = scene
        .rectangles()
        .iter()
        .map(|rectangle| (rectangle.bounds, rectangle.uv, rectangle.color))
        .collect::<Vec<_>>();
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
    assert!(scene.playfields().is_empty());
    let mut wrong = crate::scene::Scene::new(959, 720);
    let stamp = wrong.geometry_stamp().1;
    assert!(presentation.compose(&mut wrong).is_err());
    assert!(wrong.rectangles().is_empty());
    assert_eq!(wrong.geometry_stamp().1, stamp);
}
