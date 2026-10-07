//! Deferred UI/controller bridge fixtures; the bounded port is scripted, and
//! membership is produced by the actual common registry. No native I/O runs.
use super::*;
use crate::{
    multiplayer_group::{GroupPrefix, MemberProgress},
    multiplayer_group_rooms::{GroupRoomPolicy, GroupRoomRegistry},
    multiplayer_protocol::Progress,
    native_room_network::{NativeRoomReceipts, NativeRoomRoster},
    player,
    room_presentation::{RoomStatus, RoomUiAction},
};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

fn collecting() -> GroupRoomRegistry {
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 3, 8, 100).unwrap());
    for _ in 0..3 {
        registry
            .join(
                "room",
                b"actual identity",
                &[PlayerId(7), PlayerId(9), PlayerId(u32::MAX)],
                0,
            )
            .unwrap();
    }
    registry
}
fn prepared_registry() -> GroupRoomRegistry {
    let mut registry = collecting();
    let ids = registry
        .room("room")
        .unwrap()
        .members
        .iter()
        .map(|member| member.id)
        .collect::<Vec<_>>();
    registry.seal(ids[0], 1).unwrap();
    for id in ids {
        registry.ready(id, 2).unwrap();
    }
    registry
}

struct Script {
    snapshot: NativeRoomSnapshot,
    commands: Vec<(u64, NativeRoomCommand)>,
    replies: VecDeque<NativeRoomReply>,
    next: u64,
    refuse: Option<io::ErrorKind>,
    stop_requested: usize,
    joins: usize,
}
#[derive(Clone)]
struct UiPort(Rc<RefCell<Script>>);
impl NativeRoomPort for UiPort {
    fn try_command(&mut self, command: NativeRoomCommand) -> io::Result<u64> {
        let mut script = self.0.borrow_mut();
        if let Some(kind) = script.refuse.take() {
            return Err(io::Error::new(kind, "scripted bounded network admission"));
        }
        let id = script.next;
        script.next += 1;
        script.commands.push((id, command));
        Ok(id)
    }
    fn poll(&self) -> io::Result<NativeRoomPoll> {
        let mut script = self.0.borrow_mut();
        Ok(NativeRoomPoll {
            snapshot: script.snapshot.clone(),
            replies: script.replies.drain(..).collect(),
        })
    }
    fn clock_now_ns(&self) -> io::Result<i64> {
        Ok(10)
    }
    fn request_stop(&self) {
        self.0.borrow_mut().stop_requested += 1;
    }
    fn stop(&mut self) -> NativeRoomOutcome {
        let mut script = self.0.borrow_mut();
        script.joins += 1;
        let outcome = script
            .snapshot
            .terminal
            .clone()
            .unwrap_or(NativeRoomOutcome {
                cancelled: true,
                error: None,
                cleanup_error: None,
                receipts: NativeRoomReceipts::default(),
                leave_written: false,
            });
        script.snapshot.terminal = Some(outcome.clone());
        outcome
    }
}
fn controller(
    registry: &GroupRoomRegistry,
) -> (NativeRoomCompetition<UiPort>, Rc<RefCell<Script>>) {
    let snapshot = state(registry);
    let players = snapshot.room.as_ref().unwrap().members[0].players.clone();
    let script = Rc::new(RefCell::new(Script {
        snapshot,
        commands: Vec::new(),
        replies: VecDeque::new(),
        next: 101,
        refuse: None,
        stop_requested: 0,
        joins: 0,
    }));
    (
        NativeRoomCompetition::new(UiPort(script.clone()), players, Duration::from_secs(1))
            .unwrap(),
        script,
    )
}

#[test]
fn actual_ui_dispatch_correlates_distinct_network_ids_and_confirms_pages_without_network_commands()
{
    let mut registry = collecting();
    let (mut owner, script) = controller(&registry);
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        owner.poll().unwrap();
        let initial = viewer.take_latest().unwrap().room.unwrap();
        assert!(initial.allows(RoomUiAction::Seal));
        let ui_seal = viewer.request_room(RoomUiAction::Seal).unwrap();
        owner.poll().unwrap();
        assert_eq!(script.borrow().commands, [(101, NativeRoomCommand::Seal)]);
        assert_ne!(ui_seal, 101);
        assert!(viewer.take_room_reply().unwrap().is_none());
        let ids = registry
            .room("room")
            .unwrap()
            .members
            .iter()
            .map(|member| member.id)
            .collect::<Vec<_>>();
        registry.seal(ids[0], 1).unwrap();
        script.borrow_mut().snapshot = state(&registry);
        script.borrow_mut().replies.push_back(NativeRoomReply {
            id: 101,
            result: Ok(()),
        });
        owner.poll().unwrap();
        let reply = viewer.take_room_reply().unwrap().unwrap();
        assert_eq!(reply.id, ui_seal);
        assert!(reply.result.is_ok());
        let ui_ready = viewer.request_room(RoomUiAction::Ready).unwrap();
        owner.poll().unwrap();
        assert_eq!(
            script.borrow().commands.last().unwrap(),
            &(102, NativeRoomCommand::Ready)
        );
        let stale_ready = viewer.request_room(RoomUiAction::Ready).unwrap();
        registry.ready(ids[0], 2).unwrap();
        for id in &ids[1..] {
            registry.ready(*id, 2).unwrap();
        }
        script.borrow_mut().snapshot = state(&registry);
        script.borrow_mut().replies.push_back(NativeRoomReply {
            id: 102,
            result: Ok(()),
        });
        owner.poll().unwrap();
        assert_eq!(viewer.take_room_reply().unwrap().unwrap().id, ui_ready);
        let refusal = viewer.take_room_reply().unwrap().unwrap();
        assert_eq!(refusal.id, stale_ready);
        assert!(refusal.result.is_err());
        assert_eq!(
            script.borrow().commands.len(),
            2,
            "a request admitted by old UI metadata is rechecked before network dispatch"
        );
        let before = viewer.take_latest().unwrap().room.unwrap();
        assert_eq!((before.page, before.pages), (0, 2));
        let page = viewer.request_room(RoomUiAction::Page(1)).unwrap();
        assert_eq!(owner.hud().unwrap().page_index(), 0);
        owner.poll().unwrap();
        let reply = viewer.take_room_reply().unwrap().unwrap();
        assert_eq!(reply.id, page);
        assert!(reply.result.is_ok());
        assert_eq!(viewer.take_latest().unwrap().room.unwrap().page, 1);
        assert_eq!(script.borrow().commands.len(), 2);
        assert!(viewer.request_room(RoomUiAction::Page(2)).is_err());
        owner.poll().unwrap();
        assert!(viewer.take_room_reply().unwrap().is_none());
        assert_eq!(owner.hud().unwrap().page_index(), 1);
        assert!(owner.network_error().is_none());
        assert!(viewer.request_room(RoomUiAction::Seal).is_err());
        owner.poll().unwrap();
        assert!(viewer.take_room_reply().unwrap().is_none());
        assert_eq!(script.borrow().commands.len(), 2);
        script.borrow_mut().refuse = Some(io::ErrorKind::WouldBlock);
        let leave = viewer.request_room(RoomUiAction::Leave).unwrap();
        owner.poll().unwrap();
        let reply = viewer.take_room_reply().unwrap().unwrap();
        assert_eq!(reply.id, leave);
        assert!(reply.result.is_err());
        assert!(owner.network_error().is_none());
        assert_eq!(script.borrow().stop_requested, 0);
        owner.finish(&[], false);
        assert_eq!(script.borrow().joins, 1);
        Ok(())
    })
    .unwrap();
}

#[test]
fn cancellation_and_pending_leave_fence_dispatch_while_last_peer_prefix_survives_network_failure() {
    let registry = collecting();
    let (mut owner, script) = controller(&registry);
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        owner.poll().unwrap();
        let request = viewer.request_room(RoomUiAction::Seal).unwrap();
        viewer.cancel();
        let _ = owner.poll();
        assert!(script.borrow().commands.is_empty());
        owner.finish(&[], false);
        assert_eq!(script.borrow().joins, 1);
        let reply = viewer.take_room_reply().unwrap().unwrap();
        assert_eq!(reply.id, request);
        assert!(reply.result.is_err());
        assert!(viewer.take_room_reply().unwrap().is_none());
        Ok(())
    })
    .unwrap();

    let registry = prepared_registry();
    let (mut owner, script) = controller(&registry);
    let room = registry.room("room").unwrap();
    let remote = &room.members[1];
    let prefix = Arc::new(GroupPrefix {
        sequence: 1,
        final_prefix: false,
        members: remote
            .players
            .iter()
            .map(|&player| MemberProgress {
                player,
                progress: Progress {
                    song_ns: 604_800_000_000_000,
                    hits: u64::MAX,
                    misses: 0,
                    combo: u64::MAX,
                    max_combo: u64::MAX,
                },
            })
            .collect(),
    });
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        owner.poll().unwrap();
        let request = viewer.request_room(RoomUiAction::Leave).unwrap();
        owner.poll().unwrap();
        assert_eq!(script.borrow().commands, [(101, NativeRoomCommand::Leave)]);
        assert!(owner.committed_schedule().is_err());
        script
            .borrow_mut()
            .snapshot
            .peers
            .push((remote.id, prefix.clone()));
        script.borrow_mut().snapshot.terminal = Some(NativeRoomOutcome {
            cancelled: false,
            error: Some(NativeRoomFailure {
                kind: io::ErrorKind::ConnectionReset,
                message: "connection closed".into(),
            }),
            cleanup_error: None,
            receipts: NativeRoomReceipts::default(),
            leave_written: false,
        });
        script.borrow_mut().replies.push_back(NativeRoomReply {
            id: 101,
            result: Err(NativeRoomFailure {
                kind: io::ErrorKind::ConnectionReset,
                message: "Leave was not written".into(),
            }),
        });
        assert!(owner.poll().is_err());
        let reply = viewer.take_room_reply().unwrap().unwrap();
        assert_eq!(reply.id, request);
        assert!(reply.result.is_err());
        let dto = viewer.take_latest().unwrap().room.unwrap();
        assert_eq!(dto.status, RoomStatus::Disconnected);
        assert_eq!(dto.rows[0].progress, Some(prefix.members[0].progress));
        assert_eq!(dto.rows[0].participant, remote.id);
        let own = remote
            .players
            .iter()
            .map(|&player| MemberProgress {
                player,
                progress: Progress {
                    song_ns: 604_800_000_000_001,
                    hits: 1,
                    misses: 0,
                    combo: 1,
                    max_combo: 1,
                },
            })
            .collect::<Vec<_>>();
        owner.observe(&own).unwrap();
        assert_eq!(owner.local_progress(), Some(own.as_slice()));
        assert_eq!(script.borrow().commands.len(), 1);
        let outcome = owner.finish(&own, false);
        assert!(outcome.error.is_some());
        assert!(!outcome.leave_written);
        assert_eq!(script.borrow().joins, 1);
        assert!(viewer.take_room_reply().unwrap().is_none());
        Ok(())
    })
    .unwrap();
}

fn state(registry: &GroupRoomRegistry) -> NativeRoomSnapshot {
    let room = registry.room("room").unwrap();
    NativeRoomSnapshot {
        revision: match room.phase {
            crate::multiplayer_group_rooms::GroupRoomPhase::Collecting => 1,
            crate::multiplayer_group_rooms::GroupRoomPhase::Frozen => 2,
            crate::multiplayer_group_rooms::GroupRoomPhase::Prepared => 3,
        },
        participant: Some(room.members[0].id),
        room: Some(Arc::new(NativeRoomRoster {
            members: room.members.to_vec(),
            phase: room.phase,
            deadline_ns: room.deadline_ns,
        })),
        ..NativeRoomSnapshot::default()
    }
}
