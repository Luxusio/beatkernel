//! Deferred exact historical association; completion remains a separate type.
use crate::{
    record_association::*,
    result_archive::{ResultArchive, ArchivedResult, decode_archive},
    local_players::PlayerId,
    gauge::BmsGauge,
    play_result::CompletedPlayResult,
};
use beatkernel::{
    replay::ReplayHeader,
    time::{ClockDomainId, Timestamp},
};
pub(crate) fn archive(
    headers: &[(PlayerId, ReplayHeader)],
    start: i64,
    end: Option<i64>,
) -> ResultArchive {
    let gauge = BmsGauge::default();
    let result = CompletedPlayResult::from_completed(
        Timestamp::from_nanos(start),
        end.map(Timestamp::from_nanos),
        &gauge,
    );
    ResultArchive::from_completed(
        &headers
            .iter()
            .map(|(id, _)| (*id, result))
            .collect::<Vec<_>>(),
        &headers
            .iter()
            .map(|(id, header)| (*id, header.clone(), gauge.profile().clone()))
            .collect::<Vec<_>>(),
    )
    .unwrap()
}
#[test]
fn every_header_field_and_explicit_original_id_must_match_exactly() {
    let header = crate::result_archive::fixtures::header(0, None);
    let archive = archive(&[(PlayerId(u32::MAX), header.clone())], 0, None);
    let row = associate(&archive, &header, Some(PlayerId(u32::MAX))).unwrap();
    let historical: ArchivedResult = row.result;
    assert_eq!(historical.gauge.level_units, 20_000_000);
    assert_eq!(row.player, PlayerId(u32::MAX));
    assert_eq!(
        associate(&archive, &header, None).unwrap().player,
        PlayerId(u32::MAX)
    );
    for field in 0..6 {
        let mut changed = header.clone();
        match field {
            0 => changed.version += 1,
            1 => changed.chart_identity.push(0),
            2 => changed.rules_identity.push(0),
            3 => changed.options.push(0),
            4 => changed.seed += 1,
            _ => changed.normalized_clock = ClockDomainId(99),
        }
        assert!(associate(&archive, &changed, Some(PlayerId(u32::MAX))).is_err());
    }
    assert!(associate(&archive, &header, Some(PlayerId(7))).is_err());
    assert!(associate(&archive, &header, Some(PlayerId(0))).is_err());
}
#[test]
fn equal_cohort_headers_require_explicit_id_instead_of_order_or_filename_guess() {
    let header = crate::result_archive::fixtures::header(0, None);
    let archive = archive(
        &[
            (PlayerId(u32::MAX), header.clone()),
            (PlayerId(7), header.clone()),
        ],
        0,
        None,
    );
    assert!(matches!(
        associate(&archive, &header, None),
        Err(AssociationError::Ambiguous)
    ));
    assert_eq!(
        associate(&archive, &header, Some(PlayerId(7)))
            .unwrap()
            .player,
        PlayerId(7)
    );
    assert_eq!(
        associate(&archive, &header, Some(PlayerId(u32::MAX)))
            .unwrap()
            .player,
        PlayerId(u32::MAX)
    );
}
#[test]
fn bad_later_archive_row_never_supplies_a_partial_association() {
    let mut bytes = crate::result_archive::fixtures::golden();
    let row = bytes[16..].to_vec();
    bytes[12..16].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&row);
    assert!(decode_archive(&bytes).is_err());
    let header = crate::result_archive::fixtures::header(0, None);
    let archive = archive(&[(PlayerId(7), header.clone())], 0, None);
    let mut missing = header.clone();
    missing.seed = u64::MAX;
    assert!(matches!(
        associate(&archive, &missing, None),
        Err(AssociationError::Missing)
    ));
}
