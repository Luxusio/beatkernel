//! Deferred common-actor fixtures: actual protocol owners, memory IO only.
use super::{RoomNetworkActor, RoomNetworkStream};
use crate::{
    local_players::PlayerId,
    multiplayer_group_rooms::{GroupRoomPolicy, GroupRoomRegistry},
    multiplayer_room_io::RoomPlayIo,
    multiplayer_room_play::RoomPlayClient,
    multiplayer_room_wire::{RoomMessage, decode_message, encode_message},
    multiplayer_start::StartPolicy,
    room_network_model::{RoomCommand, RoomFailure, RoomNetworkOptions, RoomReceipts},
};
use std::{
    cell::RefCell,
    collections::VecDeque,
    io::{self, Read, Write},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

const IDENTITY: &[u8] = &[17, 0, 91];

#[derive(Default)]
struct Script {
    input: VecDeque<u8>,
    output: Vec<u8>,
    reads: usize,
    writes: usize,
    idle: Vec<Duration>,
    finish: Vec<Duration>,
    drops: usize,
    read_error: bool,
    idle_error: bool,
    cleanup_error: bool,
}
struct Stream(Rc<RefCell<Script>>);
impl Drop for Stream {
    fn drop(&mut self) {
        self.0.borrow_mut().drops += 1;
    }
}
impl Read for Stream {
    fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
        let mut state = self.0.borrow_mut();
        state.reads += 1;
        assert!((1..=4096).contains(&target.len()));
        if state.read_error {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "original read refusal",
            ));
        }
        if state.input.is_empty() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let count = target.len().min(state.input.len());
        for byte in &mut target[..count] {
            *byte = state.input.pop_front().unwrap();
        }
        Ok(count)
    }
}
impl Write for Stream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut state = self.0.borrow_mut();
        state.writes += 1;
        state.output.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        panic!("actor must not flush");
    }
}
impl RoomNetworkStream for Stream {
    fn idle(&mut self, duration: Duration) -> io::Result<()> {
        let mut state = self.0.borrow_mut();
        state.idle.push(duration);
        if state.idle_error {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "original idle refusal",
            ))
        } else {
            Ok(())
        }
    }
    fn finish(&mut self, duration: Duration) -> io::Result<()> {
        let mut state = self.0.borrow_mut();
        state.finish.push(duration);
        if state.cleanup_error {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "separate cleanup refusal",
            ))
        } else {
            Ok(())
        }
    }
}
fn options() -> RoomNetworkOptions {
    RoomNetworkOptions {
        setup_timeout: Duration::from_secs(1),
        drain_timeout: Duration::from_millis(1),
        finish_timeout: Duration::from_millis(10),
        queue_capacity: 2,
        start_policy: StartPolicy::default(),
        preroll_ns: 0,
    }
}
fn owner_io(state: Rc<RefCell<Script>>) -> RoomPlayIo<Stream> {
    RoomPlayIo::new(
        RoomPlayClient::new(
            IDENTITY,
            &[PlayerId(u32::MAX), PlayerId(7)],
            options().start_policy,
            0,
        )
        .unwrap(),
        Stream(state),
    )
}
fn actor() -> (RoomNetworkActor<Stream>, Rc<RefCell<Script>>) {
    let state = Rc::new(RefCell::new(Script::default()));
    (
        RoomNetworkActor::new(owner_io(state.clone()), options()).unwrap(),
        state,
    )
}
fn emitted(state: &Rc<RefCell<Script>>) -> RoomMessage {
    decode_message(&std::mem::take(&mut state.borrow_mut().output)).unwrap()
}
fn deliver(
    actor: &mut RoomNetworkActor<Stream>,
    state: &Rc<RefCell<Script>>,
    message: RoomMessage,
    now: i64,
) {
    state
        .borrow_mut()
        .input
        .extend(encode_message(&message).unwrap());
    for _ in 0..32 {
        if state.borrow().input.is_empty() {
            return;
        }
        let before = (state.borrow().reads, state.borrow().writes);
        assert!(actor.drive(|| Ok(now)).unwrap());
        let after = state.borrow();
        assert!(after.reads - before.0 <= 1);
        assert!(after.writes - before.1 <= 1);
    }
    panic!("bounded frame was not consumed");
}
fn room_message(registry: &GroupRoomRegistry) -> RoomMessage {
    let room = registry.room("fixture").unwrap();
    RoomMessage::Snapshot {
        members: room.members.to_vec(),
        phase: room.phase,
        deadline_ns: room.deadline_ns,
    }
}
fn admitted() -> (
    RoomNetworkActor<Stream>,
    Rc<RefCell<Script>>,
    GroupRoomRegistry,
) {
    let (mut actor, state) = actor();
    assert!(actor.drive(|| Ok(0)).unwrap());
    let RoomMessage::Join { identity, players } = emitted(&state) else {
        panic!("actual Join required")
    };
    assert_eq!(identity, IDENTITY);
    assert_eq!(players, [PlayerId(u32::MAX), PlayerId(7)]);
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 2, 16, 1_000).unwrap());
    let id = registry.join("fixture", &identity, &players, 0).unwrap().id;
    deliver(
        &mut actor,
        &state,
        RoomMessage::Admitted { participant: id },
        0,
    );
    deliver(&mut actor, &state, room_message(&registry), 0);
    (actor, state, registry)
}

#[test]
fn constructor_validation_precedes_every_stream_operation() {
    for field in 0..5 {
        let state = Rc::new(RefCell::new(Script::default()));
        let mut config = options();
        match field {
            0 => config.setup_timeout = Duration::ZERO,
            1 => config.drain_timeout = Duration::from_secs(121),
            2 => config.finish_timeout = Duration::ZERO,
            3 => config.queue_capacity = 0,
            _ => config.preroll_ns = -1,
        }
        let result = RoomNetworkActor::new(owner_io(state.clone()), config);
        assert!(matches!(result, Err(ref error) if error.kind() == io::ErrorKind::InvalidInput));
        let state = state.borrow();
        assert_eq!((state.reads, state.writes), (0, 0));
        assert!(state.idle.is_empty() && state.finish.is_empty());
        assert_eq!(state.drops, 1);
    }
}

#[test]
fn refused_commands_and_regressing_observations_keep_prior_evidence() {
    let (mut actor, state) = actor();
    for command in [RoomCommand::Seal, RoomCommand::Ready, RoomCommand::Drain] {
        assert_eq!(
            actor.command(command, -1).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
    assert!(actor.command(RoomCommand::Drain, 100).is_err());
    // Refusing an uncommitted drain must not establish its supplied time.
    assert!(actor.drive(|| Ok(99)).unwrap());
    let original = actor.snapshot().clone();
    let calls = (state.borrow().reads, state.borrow().writes);
    for now in [-1, 98] {
        let error = actor.drive(|| Ok(now)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "native room observation is negative or regressing"
        );
        assert_eq!(actor.snapshot(), &original);
        assert_eq!((state.borrow().reads, state.borrow().writes), calls);
    }
    assert!(!actor.finished() && !actor.leave_written());
}

#[test]
fn fixed_setup_deadline_counts_post_io_crossing_and_preserves_original_io_error() {
    let (mut exact, state) = actor();
    assert!(exact.drive(|| Ok(0)).unwrap());
    emitted(&state);
    let calls = (state.borrow().reads, state.borrow().writes);
    assert_eq!(
        exact.drive(|| Ok(1_000_000_000)).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    assert_eq!((state.borrow().reads, state.borrow().writes), calls);

    for read_failure in [false, true] {
        let (mut crossed, state) = actor();
        state.borrow_mut().read_error = read_failure;
        let mut reads = 0;
        let error = crossed
            .drive(|| {
                reads += 1;
                Ok(if reads <= 2 {
                    999_999_999
                } else {
                    1_000_000_000
                })
            })
            .unwrap_err();
        assert!(matches!(emitted(&state), RoomMessage::Join { .. }));
        if read_failure {
            assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
            assert_eq!(error.to_string(), "original read refusal");
        } else {
            assert_eq!(error.kind(), io::ErrorKind::TimedOut);
            assert_eq!(
                error.to_string(),
                "native room setup deadline expired during I/O"
            );
        }
        state.borrow_mut().cleanup_error = true;
        let original_kind = error.kind();
        let outcome = crossed.finish(Some(RoomFailure::from(error)), false);
        assert_eq!(outcome.error.unwrap().kind, original_kind);
        assert_eq!(
            outcome.cleanup_error.unwrap().message,
            "separate cleanup refusal"
        );
        assert_eq!(outcome.receipts, RoomReceipts::default());
        assert_eq!(state.borrow().finish, [Duration::from_millis(10)]);
        assert_eq!(state.borrow().drops, 1);
    }
}

#[test]
fn actual_admission_retains_roster_arcs_and_consumes_dirty_notification_once() {
    let (mut actor, state, mut registry) = admitted();
    let original = actor.snapshot().clone();
    assert_eq!(original.revision, 1);
    assert_eq!(
        original.room.as_ref().unwrap().members[0].players,
        [PlayerId(u32::MAX), PlayerId(7)]
    );
    assert!(actor.take_changed_snapshot().is_some());
    assert!(actor.take_changed_snapshot().is_none());
    deliver(&mut actor, &state, room_message(&registry), 1);
    assert!(Arc::ptr_eq(
        original.room.as_ref().unwrap(),
        actor.snapshot().room.as_ref().unwrap()
    ));
    assert_eq!(actor.snapshot().revision, 1);
    assert!(actor.take_changed_snapshot().is_none());
    registry
        .join("fixture", IDENTITY, &[PlayerId(91)], 1)
        .unwrap();
    deliver(&mut actor, &state, room_message(&registry), 1);
    assert_eq!(actor.snapshot().revision, 2);
    assert_eq!(original.room.unwrap().members.len(), 1);
    assert_eq!(actor.snapshot().room.as_ref().unwrap().members.len(), 2);
    assert!(actor.take_changed_snapshot().is_some());
    assert!(actor.take_changed_snapshot().is_none());
    assert!(actor.snapshot().schedule.is_none());
    assert_eq!(actor.snapshot().receipts, RoomReceipts::default());
}

#[test]
fn injected_idle_and_cleanup_diagnostics_remain_independent_of_cancellation() {
    let (mut actor, state) = actor();
    state.borrow_mut().idle_error = true;
    state.borrow_mut().cleanup_error = true;
    let error = actor.idle(Duration::from_nanos(37)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert_eq!(error.to_string(), "original idle refusal");
    let outcome = actor.finish(Some(error.into()), true);
    assert!(outcome.cancelled);
    assert_eq!(outcome.error.unwrap().message, "original idle refusal");
    assert_eq!(
        outcome.cleanup_error.unwrap().kind,
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(outcome.receipts, RoomReceipts::default());
    assert!(!outcome.leave_written);
    let state = state.borrow();
    assert_eq!(state.idle, [Duration::from_nanos(37)]);
    assert_eq!(state.finish, [Duration::from_millis(10)]);
    assert_eq!((state.reads, state.writes, state.drops), (0, 0, 1));
}

#[test]
fn actual_leave_write_stops_driving_without_fabricating_final_receipts() {
    let (mut actor, state, _) = admitted();
    actor.command(RoomCommand::Leave, 2).unwrap();
    assert!(actor.drive(|| Ok(2)).unwrap());
    assert_eq!(emitted(&state), RoomMessage::Leave);
    assert!(actor.finished() && actor.leave_written());
    assert!(
        !actor
            .drive(|| panic!("finished Leave must not acquire a clock"))
            .unwrap()
    );
    assert!(actor.command(RoomCommand::Ready, 2).is_err());
    let outcome = actor.finish(None, true);
    assert!(outcome.cancelled && outcome.leave_written);
    assert!(outcome.error.is_none() && outcome.cleanup_error.is_none());
    assert_eq!(outcome.receipts, RoomReceipts::default());
    assert_eq!(state.borrow().finish.len(), 1);
    assert_eq!(state.borrow().drops, 1);
}
