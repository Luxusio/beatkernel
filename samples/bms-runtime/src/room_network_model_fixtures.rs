use super::*;
use crate::{
    local_players::PlayerId,
    multiplayer_group::{GroupPrefix, MemberProgress},
    multiplayer_group_rooms::{GroupRoomMember, GroupRoomPhase},
    multiplayer_protocol::Progress,
    multiplayer_rooms::ParticipantId,
    multiplayer_start::{StartPolicy, StartSchedule},
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
};

fn receipts(mask: u8) -> RoomReceipts {
    RoomReceipts {
        local_final_written: mask & 1 != 0,
        local_final_acknowledged: mask & 2 != 0,
        progress_complete: mask & 4 != 0,
        drain_complete: mask & 8 != 0,
    }
}
fn failure(kind: io::ErrorKind, message: &str) -> RoomFailure {
    RoomFailure {
        kind,
        message: message.into(),
    }
}
fn outcome() -> RoomOutcome {
    RoomOutcome {
        cancelled: true,
        error: Some(failure(
            io::ErrorKind::BrokenPipe,
            "original network refusal",
        )),
        cleanup_error: Some(failure(io::ErrorKind::TimedOut, "original join refusal")),
        receipts: receipts(5),
        leave_written: false,
    }
}
fn rows() -> Vec<MemberProgress> {
    vec![
        MemberProgress {
            player: PlayerId(u32::MAX),
            progress: Progress {
                song_ns: 9_007_199_254_740_993,
                hits: u64::MAX,
                misses: 0,
                combo: u64::MAX,
                max_combo: u64::MAX,
            },
        },
        MemberProgress {
            player: PlayerId(7),
            progress: Progress {
                song_ns: -604_800_000_000_001,
                hits: 17,
                misses: 3,
                combo: 9,
                max_combo: 15,
            },
        },
    ]
}
fn snapshot() -> RoomSnapshot {
    let roster = Arc::new(RoomRoster {
        members: vec![
            GroupRoomMember {
                id: ParticipantId(u64::MAX),
                players: vec![PlayerId(u32::MAX), PlayerId(7)],
                prepared: true,
            },
            GroupRoomMember {
                id: ParticipantId(7),
                players: vec![PlayerId(91)],
                prepared: false,
            },
        ],
        phase: GroupRoomPhase::Frozen,
        deadline_ns: Some(604_800_000_000_001),
    });
    RoomSnapshot {
        revision: u64::MAX,
        participant: Some(ParticipantId(u64::MAX)),
        room: Some(roster),
        schedule: Some(StartSchedule {
            target_ns: 9_007_199_254_740_993,
            song_target_ns: i64::MAX,
            uncertainty_ns: u64::MAX,
        }),
        peers: vec![(
            ParticipantId(7),
            Arc::new(GroupPrefix {
                sequence: u64::MAX,
                final_prefix: true,
                members: rows(),
            }),
        )],
        receipts: receipts(10),
        terminal: None,
    }
}

#[test]
fn option_defaults_and_all_inclusive_time_queue_preroll_bounds_are_portable() {
    let default = RoomNetworkOptions::default();
    assert_eq!(default.setup_timeout, Duration::from_secs(60));
    assert_eq!(default.drain_timeout, Duration::from_secs(10));
    assert_eq!(default.frame_timeout, Duration::from_secs(10));
    assert_eq!(default.finish_timeout, Duration::from_secs(2));
    assert_eq!(default.queue_capacity, 32);
    assert_eq!(default.start_policy, StartPolicy::default());
    assert_eq!(default.preroll_ns, 0);
    assert!(default.validate().is_ok());
    let upper_policy = RoomNetworkOptions {
        start_policy: StartPolicy {
            lead_ns: i64::MAX as u64,
            min_remaining_ns: 1,
            max_age_ns: i64::MAX as u64,
            max_uncertainty_ns: i64::MAX as u64,
            max_release_lateness_ns: i64::MAX as u64,
        },
        ..default
    };
    assert!(upper_policy.validate().is_ok());
    for setup in [1, 120_000] {
        for drain in [1, 120_000] {
            for finish in [1, 120_000] {
                for queue in [1, 1024] {
                    for preroll in [0, i64::MAX] {
                        let options = RoomNetworkOptions {
                            setup_timeout: Duration::from_millis(setup),
                            drain_timeout: Duration::from_millis(drain),
                            finish_timeout: Duration::from_millis(finish),
                            frame_timeout: Duration::from_secs(10),
                            queue_capacity: queue,
                            preroll_ns: preroll,
                            start_policy: StartPolicy {
                                lead_ns: 2,
                                min_remaining_ns: 1,
                                max_age_ns: 0,
                                max_uncertainty_ns: 0,
                                max_release_lateness_ns: 0,
                            },
                        };
                        assert!(options.validate().is_ok());
                    }
                }
            }
        }
    }
    for frame in [Duration::from_millis(1), Duration::from_secs(120)] {
        assert!(
            RoomNetworkOptions {
                frame_timeout: frame,
                ..default
            }
            .validate()
            .is_ok()
        );
    }
}

#[test]
fn every_invalid_option_field_and_policy_bound_refuses_without_acquisition() {
    for case in 0..19 {
        let mut options = RoomNetworkOptions::default();
        match case {
            0 => options.setup_timeout = Duration::ZERO,
            1 => options.setup_timeout = Duration::from_millis(120_001),
            2 => options.drain_timeout = Duration::ZERO,
            3 => options.drain_timeout = Duration::from_millis(120_001),
            4 => options.finish_timeout = Duration::ZERO,
            5 => options.finish_timeout = Duration::from_millis(120_001),
            6 => options.queue_capacity = 0,
            7 => options.queue_capacity = 1025,
            8 => options.preroll_ns = -1,
            9 => options.start_policy.min_remaining_ns = 0,
            10 => options.start_policy.lead_ns = options.start_policy.min_remaining_ns,
            11 => options.start_policy.lead_ns = options.start_policy.min_remaining_ns - 1,
            12 => options.start_policy.lead_ns = u64::MAX,
            13 => options.start_policy.min_remaining_ns = u64::MAX,
            14 => options.start_policy.max_age_ns = u64::MAX,
            15 => options.start_policy.max_uncertainty_ns = u64::MAX,
            16 => options.start_policy.max_release_lateness_ns = u64::MAX,
            17 => options.frame_timeout = Duration::ZERO,
            _ => options.frame_timeout = Duration::from_millis(120_001),
        }
        assert_eq!(
            options.validate().unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}

#[test]
fn full_width_snapshot_values_and_shared_roster_prefix_allocations_survive_clone_exactly() {
    // These are retained data, including adversarial schedule bounds, not admitted Commit evidence.
    let original = snapshot();
    let cloned = original.clone();
    assert_eq!(cloned, original);
    assert!(Arc::ptr_eq(
        original.room.as_ref().unwrap(),
        cloned.room.as_ref().unwrap()
    ));
    assert!(Arc::ptr_eq(&original.peers[0].1, &cloned.peers[0].1));
    assert_eq!(cloned.revision, u64::MAX);
    assert_eq!(cloned.participant, Some(ParticipantId(u64::MAX)));
    assert_eq!(
        cloned.room.as_ref().unwrap().members[0].players,
        [PlayerId(u32::MAX), PlayerId(7)]
    );
    assert_eq!(
        cloned.room.as_ref().unwrap().deadline_ns,
        Some(604_800_000_000_001)
    );
    let schedule = cloned.schedule.unwrap();
    assert_eq!(schedule.target_ns, 9_007_199_254_740_993);
    assert_eq!(schedule.song_target_ns, i64::MAX);
    assert_eq!(schedule.uncertainty_ns, u64::MAX);
    assert_eq!(cloned.peers[0].0, ParticipantId(7));
    assert_eq!(cloned.peers[0].1.sequence, u64::MAX);
    assert_eq!(cloned.peers[0].1.members, rows());
    assert_eq!(cloned.receipts, receipts(10));
    assert_eq!(cloned.terminal, None);
}

#[test]
fn every_receipt_mask_retains_cancel_leave_network_and_cleanup_history_independently() {
    for mask in 0..16 {
        for cancelled in [false, true] {
            for failed in [false, true] {
                for cleanup in [false, true] {
                    for leave_written in [false, true] {
                        let network_error =
                            failed.then(|| failure(io::ErrorKind::BrokenPipe, "network history"));
                        let cleanup_error =
                            cleanup.then(|| failure(io::ErrorKind::TimedOut, "cleanup history"));
                        let original = RoomOutcome {
                            cancelled,
                            error: network_error.clone(),
                            cleanup_error: cleanup_error.clone(),
                            receipts: receipts(mask),
                            leave_written,
                        };
                        let retained = original.clone();
                        assert_eq!(retained, original);
                        assert_eq!(retained.cancelled, cancelled);
                        assert_eq!(retained.leave_written, leave_written);
                        assert_eq!(retained.receipts, receipts(mask));
                        assert_eq!(retained.error, network_error);
                        assert_eq!(retained.cleanup_error, cleanup_error);
                        let value = RoomSnapshot {
                            terminal: Some(retained.clone()),
                            receipts: receipts(mask),
                            ..RoomSnapshot::default()
                        };
                        assert_eq!(value.clone().terminal, Some(retained));
                        assert_eq!(value.schedule, None);
                    }
                }
            }
        }
    }
}

struct Port {
    commands: Vec<(u64, RoomCommand)>,
    replies: RefCell<VecDeque<RoomReply>>,
    next: u64,
    snapshot: RoomSnapshot,
    now: i64,
    stops: Cell<usize>,
    joins: usize,
    joined: RoomOutcome,
}
impl RoomNetworkPort for Port {
    fn try_command(&mut self, command: RoomCommand) -> io::Result<u64> {
        let id = self.next;
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| io::Error::other("scripted ID exhausted"))?;
        self.commands.push((id, command));
        Ok(id)
    }
    fn poll(&self) -> io::Result<RoomPoll> {
        Ok(RoomPoll {
            snapshot: self.snapshot.clone(),
            replies: self.replies.borrow_mut().drain(..).collect(),
        })
    }
    fn clock_now_ns(&self) -> io::Result<i64> {
        Ok(self.now)
    }
    fn request_stop(&self) {
        self.stops.set(self.stops.get() + 1);
    }
    fn stop(&mut self) -> RoomOutcome {
        self.joins += 1;
        self.joined.clone()
    }
}

#[test]
fn pure_port_contract_accepts_owned_commands_and_preserves_fifo_evidence_and_joined_diagnostics() {
    let mut port = Port {
        commands: vec![],
        replies: RefCell::new(VecDeque::new()),
        next: 9_007_199_254_740_993,
        snapshot: snapshot(),
        now: i64::MAX,
        stops: Cell::new(0),
        joins: 0,
        joined: outcome(),
    };
    let members = rows();
    let pointer = members.as_ptr();
    let first = port
        .try_command(RoomCommand::Publish {
            members,
            final_prefix: true,
        })
        .unwrap();
    let second = port.try_command(RoomCommand::Drain).unwrap();
    assert_eq!(first, 9_007_199_254_740_993);
    assert_eq!(second, 9_007_199_254_740_994);
    match &port.commands[0].1 {
        RoomCommand::Publish {
            members,
            final_prefix,
        } => {
            assert_eq!(members.as_ptr(), pointer);
            assert_eq!(*members, rows());
            assert!(*final_prefix);
        }
        _ => panic!("portable command must preserve the moved Publish vector"),
    }
    let replies = [
        RoomReply {
            id: first,
            result: Ok(()),
        },
        RoomReply {
            id: second,
            result: Err(failure(
                io::ErrorKind::BrokenPipe,
                "original command refusal",
            )),
        },
    ];
    port.replies.borrow_mut().extend(replies.clone());
    let observed = port.poll().unwrap();
    assert_eq!(observed.replies, replies);
    assert!(Arc::ptr_eq(
        observed.snapshot.room.as_ref().unwrap(),
        port.snapshot.room.as_ref().unwrap()
    ));
    assert!(port.poll().unwrap().replies.is_empty());
    assert_eq!(port.clock_now_ns().unwrap(), i64::MAX);
    port.request_stop();
    assert_eq!(port.stops.get(), 1);
    assert_eq!(port.joins, 0);
    let joined = port.stop();
    assert_eq!(joined, outcome());
    assert_eq!(port.joins, 1);
    assert_eq!(joined.receipts, receipts(5));
    assert_eq!(
        joined.cleanup_error.unwrap().message,
        "original join refusal"
    );
}
