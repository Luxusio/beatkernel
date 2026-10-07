// Deferred child of the existing controller fixtures. The scripted port supplies
// lifecycle timing; accepted prefixes and completion receipts use real owners.
use super::*;
use crate::room_presentation::{RoomStatus, RoomUiAction};

struct AfterJoinPort {
    inner: Port,
    accepted_at_join: Vec<(crate::multiplayer_rooms::ParticipantId, Arc<GroupPrefix>)>,
}
impl NativeRoomPort for AfterJoinPort {
    fn try_command(&mut self, command: NativeRoomCommand) -> io::Result<u64> {
        self.inner.try_command(command)
    }
    fn poll(&self) -> io::Result<NativeRoomPoll> {
        self.inner.poll()
    }
    fn clock_now_ns(&self) -> io::Result<i64> {
        self.inner.clock_now_ns()
    }
    fn request_stop(&self) {
        self.inner.request_stop();
    }
    fn stop(&mut self) -> NativeRoomOutcome {
        let result = self.inner.stop();
        self.inner.0.borrow_mut().snapshot.peers = self.accepted_at_join.clone();
        result
    }
}

fn accepted_before_and_after(registry: &GroupRoomRegistry) -> (Arc<GroupPrefix>, Arc<GroupPrefix>) {
    let room = registry.room("room").unwrap();
    let own = room.members[0].id;
    let remote = &room.members[1];
    let mut sender = RoomProgressClient::new(room, remote.id).unwrap();
    let mut receiver = RoomProgressClient::new(room, own).unwrap();
    let mut relay = RoomProgressRelay::new(room).unwrap();
    sender.activate().unwrap();
    relay.activate().unwrap();
    let mut evidence = Vec::new();
    for (now, hits, final_prefix) in [(10, 6, false), (20, 7, true)] {
        sender
            .publish(&rows(&remote.players, hits), final_prefix)
            .unwrap();
        let upload = sender.poll_write(now).unwrap().unwrap();
        relay
            .receive_at(remote.id, &decode_message(&upload.bytes).unwrap(), now)
            .unwrap();
        sender.written(upload.id).unwrap();
        let forwarded = relay.poll_write_at(own, now).unwrap().unwrap();
        receiver
            .receive(&decode_message(&forwarded.bytes).unwrap(), now)
            .unwrap();
        relay.written(own, forwarded.id).unwrap();
        evidence.push(Arc::new(receiver.peer_progress(remote.id).unwrap().clone()));
    }
    assert!(!receiver.local_complete());
    (evidence.remove(0), evidence.remove(0))
}

#[test]
fn joined_controller_archives_last_accepted_prefix_once_and_releases_owner_independently() {
    for hosts in [2, 3, 4] {
        let registry = prepared(hosts, 3);
        let room = registry.room("room").unwrap();
        let (before, after) = accepted_before_and_after(&registry);
        let mut initial = snapshot(&registry);
        initial.peers.push((room.members[1].id, before.clone()));
        let (port, script) = port(initial);
        let mut owner = NativeRoomCompetition::new(
            AfterJoinPort {
                inner: port,
                accepted_at_join: vec![(room.members[1].id, after.clone())],
            },
            room.members[0].players.clone(),
            Duration::from_secs(1),
        )
        .unwrap();
        let (publisher, viewer) = player::channel();
        let archive = player::with_publisher(publisher, || {
            owner.poll().unwrap();
            if hosts == 4 {
                let mut disconnected = joined_outcome(NativeRoomReceipts::default());
                disconnected.cancelled = true;
                disconnected.error = Some(failure(
                    io::ErrorKind::ConnectionReset,
                    "failed before joined final poll",
                ));
                script.borrow_mut().snapshot.terminal = Some(disconnected);
                let _ = owner.poll();
                assert!(owner.network_error().is_some());
                assert_eq!(
                    owner.hud().unwrap().rows()[0].progress,
                    Some(before.members[0].progress)
                );
            }
            owner.set_page(((hosts - 1) * 3).div_ceil(4) - 1).unwrap();
            assert!(viewer.take_latest().unwrap().room_results.is_none());
            let result = owner.finish(&[], false);
            assert!(result.cancelled);
            assert_eq!(result.receipts, NativeRoomReceipts::default());
            assert!(!result.leave_written);
            assert_eq!(script.borrow().joined, 1);
            assert!(script.borrow().commands.is_empty());
            let published = viewer.take_latest().unwrap();
            let archive = published.room_results.unwrap();
            assert!(archive.cancelled());
            if hosts == 4 {
                assert!(
                    archive
                        .error()
                        .unwrap()
                        .contains("failed before joined final poll")
                );
            }
            assert_eq!(archive.initial_page(), ((hosts - 1) * 3).div_ceil(4) - 1);
            assert_eq!(archive.rows()[0].progress, Some(after.members[0].progress));
            assert!(archive.rows()[0].final_prefix);
            assert_ne!(archive.rows()[0].progress, Some(before.members[0].progress));
            assert!(
                archive
                    .rows()
                    .iter()
                    .skip(3)
                    .all(|row| row.progress.is_none() && !row.final_prefix)
            );
            assert_eq!(published.room.unwrap().status, RoomStatus::Closed);
            assert!(viewer.request_room(RoomUiAction::Page(0)).is_err());
            assert_eq!(owner.finish(&[], true), result);
            assert_eq!(script.borrow().joined, 1);
            if let Some(repeated) = viewer.take_latest() {
                assert!(Arc::ptr_eq(
                    repeated.room_results.as_ref().unwrap(),
                    &archive
                ));
            }
            Ok(archive)
        })
        .unwrap();
        drop(owner);
        let terminal = viewer.take_latest().unwrap();
        assert!(Arc::ptr_eq(
            terminal.room_results.as_ref().unwrap(),
            &archive
        ));
        assert_eq!(
            terminal
                .room_results
                .as_ref()
                .unwrap()
                .project(0)
                .unwrap()
                .rows[0]
                .progress,
            Some(after.members[0].progress)
        );
        assert_eq!(script.borrow().joined, 1);
        assert!(script.borrow().commands.is_empty());
    }
}

#[test]
fn terminal_archive_preserves_real_drain_evidence_and_failed_hud_without_changing_outcome() {
    let registry = prepared(3, 3);
    let room = registry.room("room").unwrap();
    let (_, accepted) = accepted_before_and_after(&registry);
    let (local, drained) = completion_evidence(&registry);
    for disable in [false, true] {
        let mut initial = snapshot(&registry);
        initial.schedule = Some(schedules(&registry)[0]);
        initial.peers.push((room.members[1].id, accepted.clone()));
        let (mut owner, script) = controller(initial, room.members[0].players.clone());
        let actual = rows(&room.members[0].players, 7);
        let (publisher, viewer) = player::channel();
        let archive = player::with_publisher(publisher, || {
            owner.observe(&actual).unwrap();
            owner.poll().unwrap();
            owner.set_page(1).unwrap();
            if disable {
                owner.disable_hud();
            }
            let _ = viewer.take_latest();
            let mut expected = joined_outcome(drained);
            expected.cleanup_error = Some(failure(
                io::ErrorKind::BrokenPipe,
                "joined stream cleanup failed",
            ));
            script.borrow_mut().after_final = Some(local);
            script.borrow_mut().after_drain = Some(expected.clone());
            let outcome = owner.finish(&actual, true);
            assert_eq!(outcome, expected);
            assert!(outcome.error.is_none());
            assert!(outcome.cleanup_error.is_some());
            assert_eq!(owner.local_progress(), Some(actual.as_slice()));
            let archive = viewer.take_latest().unwrap().room_results.unwrap();
            assert!(!archive.cancelled());
            assert_eq!(archive.failed(), disable);
            assert_eq!(archive.initial_page(), 1);
            assert_eq!(
                archive.rows()[0].progress,
                Some(accepted.members[0].progress)
            );
            assert!(archive.rows()[0].final_prefix);
            assert!(
                archive
                    .error()
                    .unwrap()
                    .contains("joined stream cleanup failed")
            );
            if disable {
                assert!(
                    archive
                        .error()
                        .unwrap()
                        .contains("room score presentation was disabled")
                );
            }
            let view = archive.project(0).unwrap();
            assert_eq!(view.failed, disable);
            if disable {
                assert!(view.rows.is_empty());
            } else {
                assert_eq!(view.rows.len(), 4);
            }
            assert_eq!(owner.finish(&[], false), outcome);
            assert_eq!(script.borrow().joined, 1);
            assert_eq!(
                script
                    .borrow()
                    .commands
                    .iter()
                    .filter(|(_, command)| matches!(command, NativeRoomCommand::Drain))
                    .count(),
                1
            );
            Ok(archive)
        })
        .unwrap();
        drop(owner);
        assert!(Arc::ptr_eq(
            viewer.take_latest().unwrap().room_results.as_ref().unwrap(),
            &archive
        ));
        assert_eq!(
            archive.project(1).unwrap().rows.len(),
            if disable { 0 } else { 2 }
        );
    }
}
