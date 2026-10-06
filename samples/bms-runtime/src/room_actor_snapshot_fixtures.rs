//! Deferred actual actor over a cloned accepted session and a bounded memory edge.
use super::*;
use crate::{
    room_client_driver::fixtures::committed_pair,
    multiplayer_room_io::RoomPlayIo,
    multiplayer_room_wire::{RoomMessage, decode_message},
    multiplayer_group::MemberProgress,
    multiplayer_protocol::Progress,
    local_players::PlayerId,
};
use std::{cell::RefCell, rc::Rc, sync::Arc};
#[derive(Default)]
struct State {
    reads: usize,
    writes: usize,
    finishes: usize,
    bytes: Vec<u8>,
}
struct Stream(Rc<RefCell<State>>);
impl std::io::Read for Stream {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        self.0.borrow_mut().reads += 1;
        Err(io::ErrorKind::WouldBlock.into())
    }
}
impl std::io::Write for Stream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut state = self.0.borrow_mut();
        state.writes += 1;
        state.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl RoomNetworkStream for Stream {
    fn idle(&mut self, _: std::time::Duration) -> io::Result<()> {
        Ok(())
    }
    fn finish(&mut self, _: std::time::Duration) -> io::Result<()> {
        self.0.borrow_mut().finishes += 1;
        Ok(())
    }
}
fn actor() -> (RoomNetworkActor<Stream>, Rc<RefCell<State>>) {
    let session = committed_pair().0[0].session_ref().unwrap().clone();
    let state = Rc::new(RefCell::new(State::default()));
    (
        RoomNetworkActor::new(
            RoomPlayIo::new(session, Stream(state.clone())),
            RoomNetworkOptions::default(),
        )
        .unwrap(),
        state,
    )
}
#[test]
fn actual_refresh_and_idle_drive_retain_room_arc_genuine_schedule_and_one_shot_changed_notification()
 {
    let (mut actor, state) = actor();
    let expected = actor.io.session().clone().take_schedule().unwrap();
    actor.refresh().unwrap();
    assert_eq!(actor.snapshot().schedule, Some(expected));
    assert_eq!(actor.snapshot().revision, 1);
    let room = actor.snapshot().room.as_ref().unwrap().clone();
    assert!(actor.take_changed_snapshot().is_some());
    assert!(actor.take_changed_snapshot().is_none());
    actor.refresh().unwrap();
    assert!(actor.take_changed_snapshot().is_none());
    assert!(Arc::ptr_eq(actor.snapshot().room.as_ref().unwrap(), &room));
    assert!(!actor.drive(|| Ok(12_000)).unwrap());
    assert!(actor.take_changed_snapshot().is_none());
    assert!(state.borrow().bytes.is_empty());
    assert_eq!(actor.snapshot().schedule, Some(expected));
    let joined = actor.finish(None, false);
    assert!(!joined.receipts.local_final_written);
    assert!(!joined.receipts.progress_complete);
    assert_eq!(state.borrow().finishes, 1);
}
#[test]
fn actual_actor_final_frame_credit_becomes_retained_receipt_history_and_refresh_error_keeps_dirty_participant()
 {
    let (mut actor, state) = actor();
    actor.refresh().unwrap();
    actor.take_changed_snapshot();
    let rows: Vec<_> = [PlayerId(u32::MAX), PlayerId(7)]
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
    actor
        .command(
            RoomCommand::Publish {
                members: rows,
                final_prefix: true,
            },
            12_000,
        )
        .unwrap();
    assert!(!actor.snapshot().receipts.local_final_written);
    assert!(actor.drive(|| Ok(12_001)).unwrap());
    assert!(matches!(
        decode_message(&state.borrow().bytes).unwrap(),
        RoomMessage::Progress(_)
    ));
    assert!(actor.snapshot().receipts.local_final_written);
    assert!(!actor.snapshot().receipts.local_final_acknowledged);
    assert!(actor.take_changed_snapshot().is_some());
    let joined = actor.finish(None, true);
    assert!(joined.receipts.local_final_written);
    assert!(!joined.receipts.local_final_acknowledged);
    assert_eq!(state.borrow().finishes, 1);
    let (mut actor, _) = self::actor();
    actor.snapshot.revision = u64::MAX;
    assert!(actor.refresh().is_err());
    assert_eq!(actor.snapshot.participant, actor.io.session().participant());
    assert!(actor.snapshot.room.is_none());
    assert!(actor.take_changed_snapshot().is_some());
    assert!(actor.take_changed_snapshot().is_none());
}
