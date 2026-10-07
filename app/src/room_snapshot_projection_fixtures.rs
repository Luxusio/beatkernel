//! Deferred pure projection from genuine accepted protocol state.
use super::*;
use crate::{
    room_client_driver::fixtures::committed_pair,
    room_network_model::{RoomOutcome, RoomReceipts},
    multiplayer_group::MemberProgress,
    multiplayer_protocol::Progress,
    local_players::PlayerId,
    multiplayer_room_wire::{RoomMessage, decode_message},
    multiplayer_room_progress::RoomProgressRelay,
};
use std::sync::Arc;
#[test]
fn accepted_room_projection_retains_order_original_ids_and_arc_when_metadata_is_unchanged() {
    let (drivers, _, ids) = committed_pair();
    let session = drivers[0].session_ref().unwrap();
    let room = session.room().unwrap();
    let mut snapshot = RoomSnapshot::default();
    assert!(refresh(&mut snapshot, session).unwrap());
    assert_eq!(snapshot.revision, 1);
    assert_eq!(snapshot.participant, Some(ids[0]));
    assert_eq!(
        snapshot.room.as_ref().unwrap().members.as_slice(),
        room.members
    );
    assert_eq!(
        snapshot.room.as_ref().unwrap().members[0].players,
        [PlayerId(u32::MAX), PlayerId(7)]
    );
    let retained = snapshot.room.as_ref().unwrap().clone();
    assert!(!refresh(&mut snapshot, session).unwrap());
    assert!(Arc::ptr_eq(snapshot.room.as_ref().unwrap(), &retained));
    assert_eq!(snapshot.revision, 1);
}
#[test]
fn projection_preserves_schedule_and_terminal_and_partial_exhaustion_notifies_accepted_participant()
{
    let (drivers, _, _) = committed_pair();
    let session = drivers[0].session_ref().unwrap();
    let schedule = crate::multiplayer_start::StartSchedule {
        target_ns: i64::MAX,
        song_target_ns: i64::MAX,
        uncertainty_ns: u64::MAX,
    };
    let terminal = RoomOutcome {
        cancelled: true,
        error: None,
        cleanup_error: None,
        receipts: RoomReceipts::default(),
        leave_written: false,
    };
    let mut snapshot = RoomSnapshot {
        revision: u64::MAX,
        schedule: Some(schedule),
        terminal: Some(terminal.clone()),
        ..Default::default()
    };
    let mut changed = false;
    assert!(refresh_with_changed(&mut snapshot, session, &mut changed).is_err());
    assert!(changed);
    assert_eq!(snapshot.participant, session.participant());
    assert_eq!(snapshot.revision, u64::MAX);
    assert!(snapshot.room.is_none());
    assert_eq!(snapshot.schedule, Some(schedule));
    assert_eq!(snapshot.terminal, Some(terminal));
}
#[test]
fn actual_relay_peer_prefix_changes_arc_by_sequence_without_changing_metadata_revision() {
    let (drivers, registry, ids) = committed_pair();
    let mut sender = drivers[0].session_ref().unwrap().clone();
    let mut receiver = drivers[1].session_ref().unwrap().clone();
    let mut relay = RoomProgressRelay::new(registry.room("driver").unwrap()).unwrap();
    relay.activate().unwrap();
    let mut snapshot = RoomSnapshot::default();
    refresh(&mut snapshot, &receiver).unwrap();
    let room = snapshot.room.as_ref().unwrap().clone();
    let rows: Vec<_> = [PlayerId(u32::MAX), PlayerId(7)]
        .into_iter()
        .map(|player| MemberProgress {
            player,
            progress: Progress {
                song_ns: 9_007_199_254_740_993,
                hits: u64::MAX,
                misses: 0,
                combo: 0,
                max_combo: u64::MAX,
            },
        })
        .collect();
    let mut prior = None;
    for sequence in 1..=2 {
        sender.publish_progress(&rows, false).unwrap();
        let upload = sender.poll_write(12_000 + sequence * 100).unwrap().unwrap();
        relay
            .receive(ids[0], &decode_message(&upload.bytes).unwrap())
            .unwrap();
        sender.written(upload.id, 12_001 + sequence * 100).unwrap();
        let peer = relay.poll_write(ids[1]).unwrap().unwrap();
        receiver
            .receive(
                decode_message(&peer.bytes).unwrap(),
                12_002 + sequence * 100,
            )
            .unwrap();
        relay.written(ids[1], peer.id).unwrap();
        assert!(refresh(&mut snapshot, &receiver).unwrap());
        assert_eq!(snapshot.revision, 1);
        assert!(Arc::ptr_eq(snapshot.room.as_ref().unwrap(), &room));
        assert_eq!(snapshot.peers[0].0, ids[0]);
        assert_eq!(snapshot.peers[0].1.sequence, sequence as u64);
        assert_eq!(snapshot.peers[0].1.members, rows);
        let current = snapshot.peers[0].1.clone();
        if let Some(old) = prior {
            assert!(!Arc::ptr_eq(&old, &current));
        }
        assert!(!refresh(&mut snapshot, &receiver).unwrap());
        assert!(Arc::ptr_eq(&snapshot.peers[0].1, &current));
        prior = Some(current);
    }
}
#[test]
fn actual_final_write_receipt_accumulates_history_without_fabricating_ack_or_schedule_consumption()
{
    let (drivers, _, _) = committed_pair();
    let mut session = drivers[0].session_ref().unwrap().clone();
    let mut snapshot = RoomSnapshot::default();
    refresh(&mut snapshot, &session).unwrap();
    let players = [PlayerId(u32::MAX), PlayerId(7)];
    let members: Vec<_> = players
        .into_iter()
        .map(|player| MemberProgress {
            player,
            progress: Progress {
                song_ns: 604_800_000_000_000,
                hits: 1,
                misses: 0,
                combo: 1,
                max_combo: 1,
            },
        })
        .collect();
    session.publish_progress(&members, true).unwrap();
    let upload = session.poll_write(12_000).unwrap().unwrap();
    assert!(matches!(
        decode_message(&upload.bytes).unwrap(),
        RoomMessage::Progress(_)
    ));
    refresh(&mut snapshot, &session).unwrap();
    assert!(!snapshot.receipts.local_final_written);
    session.written(upload.id, 12_001).unwrap();
    assert!(refresh(&mut snapshot, &session).unwrap());
    assert!(snapshot.receipts.local_final_written);
    assert!(!snapshot.receipts.local_final_acknowledged);
    assert!(!snapshot.receipts.progress_complete);
    assert!(snapshot.schedule.is_none());
    assert!(session.take_schedule().is_some());
    session.stop();
    refresh(&mut snapshot, &session).unwrap();
    assert!(snapshot.receipts.local_final_written);
}
