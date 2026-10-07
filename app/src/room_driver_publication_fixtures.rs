//! Deferred actual committed room uploads with scalar cadence and real write credit.
use super::*;
use crate::{
    local_players::PlayerId,
    multiplayer_group::{MemberProgress, encode_words},
    multiplayer_protocol::Progress,
    multiplayer_room_wire::{RoomMessage, decode_message},
    multiplayer_room_progress_client::RoomProgressClientError,
};
fn rows() -> Vec<MemberProgress> {
    [PlayerId(u32::MAX), PlayerId(7)]
        .into_iter()
        .map(|player| MemberProgress {
            player,
            progress: Progress {
                song_ns: 9_007_199_254_740_993,
                hits: u64::MAX,
                misses: 0,
                combo: u64::MAX,
                max_combo: u64::MAX,
            },
        })
        .collect()
}
fn committed() -> RoomClientDriver {
    super::fixtures::committed_pair().0.remove(0)
}
fn written(driver: &mut RoomClientDriver, now: i64, sequence: u64, final_prefix: bool) {
    let upload = driver.next_write(now).unwrap().unwrap();
    let RoomMessage::Progress(prefix) = decode_message(&upload.bytes).unwrap() else {
        panic!("genuine upload required")
    };
    assert_eq!(prefix.sequence, sequence);
    assert_eq!(prefix.final_prefix, final_prefix);
    assert_eq!(prefix.members, rows());
    driver.written(upload.id, now + 1, now + 1).unwrap();
}
#[test]
fn actual_start_uploads_first_and_exact_boundary_without_suppressed_sequence_consumption() {
    let mut driver = committed();
    let words = encode_words(&rows()).unwrap();
    assert!(
        driver
            .publish_progress_words_at(&words, false, 12_000)
            .unwrap()
    );
    written(&mut driver, 12_000, 1, false);
    assert!(!driver.publication_due(50_011_999, false).unwrap());
    assert!(
        !driver
            .publish_progress_words_at(&words, false, 50_011_999)
            .unwrap()
    );
    assert_eq!(driver.publication.last_published(), Some(12_000));
    assert!(driver.next_write(50_011_999).unwrap().is_none());
    assert!(driver.publication_due(50_012_000, false).unwrap());
    assert!(
        driver
            .publish_progress_words_at(&words, false, 50_012_000)
            .unwrap()
    );
    written(&mut driver, 50_012_000, 2, false);
    assert_eq!(driver.publication.last_published(), Some(50_012_000));
    assert!(!driver.local_final_written());
    assert!(!driver.local_final_acknowledged());
}
#[test]
fn phase_and_malformed_evidence_refuse_even_under_interval_without_marker_advance() {
    let mut fresh = RoomClientDriver::new(
        &[1],
        &[PlayerId(u32::MAX), PlayerId(7)],
        crate::multiplayer_start::StartPolicy::default(),
        0,
    )
    .unwrap();
    let words = encode_words(&rows()).unwrap();
    assert!(fresh.publication_due(12_000, false).unwrap());
    assert!(matches!(
        fresh.publish_progress_words_at(&words, false, 12_000),
        Err(RoomPlayError::InvalidState)
    ));
    assert!(!fresh.failed());
    assert_eq!(fresh.publication.last_published(), None);
    for field in 0..3 {
        let mut driver = committed();
        driver
            .publish_progress_words_at(&words, false, 12_000)
            .unwrap();
        let mut malformed = words.clone();
        match field {
            0 => {
                malformed.pop();
            }
            1 => malformed[5] = 1,
            _ => malformed[0] = 91,
        }
        assert!(
            driver
                .publish_progress_words_at(&malformed, false, 12_001)
                .is_err()
        );
        assert_eq!(driver.publication.last_published(), Some(12_000));
        assert!(driver.failed());
    }
}
#[test]
fn accepted_final_bypasses_interval_once_but_not_actual_write_or_ack_receipts() {
    let mut driver = committed();
    let words = encode_words(&rows()).unwrap();
    driver
        .publish_progress_words_at(&words, false, 12_000)
        .unwrap();
    written(&mut driver, 12_000, 1, false);
    assert!(
        driver
            .publish_progress_words_at(&words, true, 12_002)
            .unwrap()
    );
    assert!(!driver.local_final_written());
    assert!(!driver.local_final_acknowledged());
    let upload = driver.next_write(12_002).unwrap().unwrap();
    let RoomMessage::Progress(prefix) = decode_message(&upload.bytes).unwrap() else {
        panic!("final prefix upload")
    };
    assert_eq!(prefix.sequence, 2);
    assert!(prefix.final_prefix);
    assert!(!driver.local_final_written());
    driver.written(upload.id, 12_003, 12_003).unwrap();
    assert!(driver.local_final_written());
    assert!(!driver.local_final_acknowledged());
    assert!(
        driver
            .publish_progress_words_at(&words, true, 12_004)
            .is_err()
    );
    assert!(!driver.failed());
    assert_eq!(driver.publication.last_published(), Some(12_002));
    assert!(driver.next_write(12_004).unwrap().is_none());
}
#[test]
fn negative_and_regressed_hints_are_fatal_even_after_false_while_large_integer_boundaries_remain_exact()
 {
    let words = encode_words(&rows()).unwrap();
    for now in [-1, 50_011_998] {
        let mut driver = committed();
        driver
            .publish_progress_words_at(&words, false, 12_000)
            .unwrap();
        assert!(!driver.publication_due(50_011_999, false).unwrap());
        assert!(matches!(
            driver.publication_due(now, true),
            Err(RoomPlayError::Progress(
                RoomProgressClientError::InvalidObservation
            ))
        ));
        assert!(driver.failed());
        assert_eq!(driver.publication.last_published(), Some(12_000));
    }
    for origin in [
        604_800_000_000_000i64,
        9_007_199_254_740_993,
        i64::MAX - 50_000_000,
    ] {
        let mut driver = committed();
        assert!(
            driver
                .publish_progress_words_at(&words, false, origin)
                .unwrap()
        );
        assert!(!driver.publication_due(origin + 49_999_999, false).unwrap());
        assert!(driver.publication_due(origin + 50_000_000, false).unwrap());
    }
}
