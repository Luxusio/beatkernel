//! Deferred facade cache from genuine Commit and accepted metadata frames.
use super::*;
use crate::{
    room_client_driver::fixtures::committed_pair,
    multiplayer_room_wire::{RoomMessage, encode_message},
};
use std::sync::Arc;
#[test]
fn fragmented_equal_snapshot_increments_accepted_frame_counter_but_retains_content_revision_and_arc()
 {
    let mut driver = committed_pair().0.remove(0);
    let public = driver.revision();
    let first = driver.retained_snapshot().unwrap().clone();
    assert_eq!(first.revision, 1);
    assert!(public > first.revision);
    let room = driver.session_ref().unwrap().room().unwrap();
    let bytes = encode_message(&RoomMessage::Snapshot {
        members: room.members.to_vec(),
        phase: room.phase,
        deadline_ns: room.deadline_ns,
    })
    .unwrap();
    let first_part = driver.needed_bytes().unwrap();
    assert!(first_part < bytes.len());
    driver
        .receive_bytes(&bytes[..first_part], 12_000, 12_000)
        .unwrap();
    assert_eq!(driver.revision(), public);
    assert!(Arc::ptr_eq(
        driver.retained_snapshot().unwrap().room.as_ref().unwrap(),
        first.room.as_ref().unwrap()
    ));
    let mut offset = first_part;
    while offset < bytes.len() {
        let count = driver.needed_bytes().unwrap().min(bytes.len() - offset);
        driver
            .receive_bytes(&bytes[offset..offset + count], 12_001, 12_001)
            .unwrap();
        offset += count;
    }
    assert_eq!(driver.revision(), public + 1);
    let retained = driver.retained_snapshot().unwrap();
    assert_eq!(retained.revision, first.revision);
    assert!(Arc::ptr_eq(
        retained.room.as_ref().unwrap(),
        first.room.as_ref().unwrap()
    ));
}
#[test]
fn cached_metadata_never_consumes_start_permission_but_actual_take_retains_exact_schedule_once() {
    let mut driver = committed_pair().0.remove(0);
    assert!(driver.retained_snapshot().unwrap().schedule.is_none());
    let expected = driver
        .session_ref()
        .unwrap()
        .clone()
        .take_schedule()
        .unwrap();
    let actual = driver.take_start().unwrap().unwrap();
    assert_eq!(actual, expected);
    assert_eq!(driver.retained_snapshot().unwrap().schedule, Some(actual));
    assert!(driver.take_start().unwrap().is_none());
    assert_eq!(driver.retained_snapshot().unwrap().schedule, Some(actual));
}
#[test]
fn projection_exhaustion_latches_original_typed_facade_failure_and_close_cannot_revive_cache() {
    let mut driver = committed_pair().0.remove(0);
    driver.retained_snapshot.revision = u64::MAX;
    assert!(matches!(
        driver.retained_snapshot(),
        Err(RoomPlayError::IdExhausted)
    ));
    assert!(driver.failed());
    assert_eq!(driver.retained_snapshot.revision, u64::MAX);
    assert!(driver.retained_snapshot.room.is_none());
    assert!(matches!(
        driver.take_start(),
        Err(RoomPlayError::IdExhausted)
    ));
    assert!(matches!(
        driver.retained_snapshot(),
        Err(RoomPlayError::IdExhausted)
    ));
    driver.close();
    assert!(driver.retained_snapshot().is_err());
}
