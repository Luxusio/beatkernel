//! Deferred whole-table validation and exact historical member projection.
use super::*;
use crate::gauge::BmsGauge;
pub(crate) fn whole(count: u32) -> ResultArchive {
    let header = crate::result_archive::fixtures::header(0, None);
    let mut rows = Vec::new();
    let mut identities = Vec::new();
    for index in 0..count {
        let player = PlayerId(u32::MAX - index * 17);
        let profile = GaugeProfile::new(
            (20 + index as u64) * 1_000_000,
            80_000_000,
            i64::MAX,
            i64::MIN,
            false,
            vec![GradeDelta {
                grade: JudgeGrade(u32::MAX - index),
                delta: i64::MIN,
            }],
        )
        .unwrap();
        let gauge = BmsGauge::new(profile.clone());
        rows.push((
            player,
            CompletedPlayResult::from_completed(Timestamp::ZERO, None, &gauge),
        ));
        identities.push((player, header.clone(), profile));
    }
    ResultArchive::from_completed(&rows, &identities).unwrap()
}
pub(crate) fn invalid_later() -> ResultArchive {
    let mut archive = whole(2);
    archive.entries[1].player = PlayerId(0);
    archive
}
#[test]
fn every_whole_roster_member_keeps_exact_original_id_header_profile_and_historical_value() {
    for count in 1..=64 {
        let original = whole(count);
        for entry in original.entries() {
            let member = original.for_player(entry.player).unwrap();
            assert_eq!(member.entries(), std::slice::from_ref(entry));
            let encoded = encode_archive(&member).unwrap();
            let decoded = decode_archive(&encoded).unwrap();
            assert_eq!(decoded, member);
            assert_eq!(decoded.entries()[0], *entry);
            assert_eq!(encoded[12..16], 1u32.to_le_bytes());
            assert_eq!(encoded[16..20], entry.player.0.to_le_bytes());
            assert_eq!(
                crate::record_association::associate(&decoded, &entry.header, None).unwrap(),
                entry
            );
        }
        assert_eq!(original.entries().len(), count as usize);
    }
}
#[test]
fn zero_unknown_and_bad_later_original_row_refuse_before_any_projection() {
    let archive = whole(2);
    assert!(archive.for_player(PlayerId(0)).is_err());
    assert!(archive.for_player(PlayerId(7)).is_err());
    let invalid = invalid_later();
    assert!(invalid.for_player(PlayerId(u32::MAX)).is_err());
    assert!(
        crate::result_archive::fixtures::invalid_archive()
            .for_player(PlayerId(0))
            .is_err()
    );
}
#[test]
fn identical_headers_become_unambiguous_only_after_member_projection_without_id_guessing() {
    let archive = whole(2);
    let header = archive.entries()[0].header.clone();
    assert_eq!(header, archive.entries()[1].header);
    assert!(crate::record_association::associate(&archive, &header, None).is_err());
    let original = archive.entries()[1].player;
    let member = archive.for_player(original).unwrap();
    assert_eq!(
        crate::record_association::associate(&member, &header, None)
            .unwrap()
            .player,
        original
    );
    assert_eq!(member.entries()[0].profile, archive.entries()[1].profile);
}
