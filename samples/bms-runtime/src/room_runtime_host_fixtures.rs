use super::*;
use crate::{
    room_runtime_host::RoomRuntimeHost,
    room_start_wait::RoomStartWaitControl,
    final_ack_wait::FinalWaitControl,
    room_network_model::{
        RoomNetworkPort, RoomCommand, RoomPoll, RoomSnapshot, RoomOutcome, RoomFailure, RoomReply,
        RoomRoster, RoomReceipts,
    },
    room_ui_host::RoomUiHost,
    room_presentation::RoomUiRequest,
    multiplayer_group_rooms::{GroupRoomRegistry, GroupRoomPolicy},
    multiplayer_clock::{ClockFilter, ClockSample},
    multiplayer_room_start::RoomStartCoordinator,
    multiplayer_start::{StartAgreement, StartRole, StartPolicy},
    multiplayer_protocol::Progress,
};
use std::{cell::RefCell, rc::Rc};

const NETWORK_ORIGIN: i64 = 9_007_199_254_740_993;
#[derive(Debug, PartialEq, Eq)]
enum Effect {
    StartFactory,
    Service,
    Poll,
    StartWait(u64),
    NetworkClock,
    FinalFactory(Duration),
    ControlClock,
    Park(u64),
    Final,
    Drain,
    Progress,
    Stop,
    Join,
    Dropped,
}
type Trace = Rc<RefCell<Vec<Effect>>>;
struct RuntimeState {
    trace: Trace,
    start_error: Option<io::Error>,
    factory_error: Option<io::Error>,
    park_error: Option<io::Error>,
    times: VecDeque<io::Result<u64>>,
    dropped: Vec<RoomOutcome>,
    timeouts: Vec<Duration>,
}
struct RuntimeHost(Rc<RefCell<RuntimeState>>);
struct StartControl(Rc<RefCell<RuntimeState>>);
struct FinalControl(Rc<RefCell<RuntimeState>>);
impl RoomStartWaitControl for StartControl {
    type Error = io::Error;
    fn wait_ns(&mut self, ns: u64) -> io::Result<()> {
        let mut state = self.0.borrow_mut();
        state.trace.borrow_mut().push(Effect::StartWait(ns));
        state.start_error.take().map_or(Ok(()), Err)
    }
}
impl FinalWaitControl for FinalControl {
    type Error = io::Error;
    fn now_ns(&mut self) -> io::Result<u64> {
        let mut state = self.0.borrow_mut();
        state.trace.borrow_mut().push(Effect::ControlClock);
        state
            .times
            .pop_front()
            .expect("unexpected final control sample")
    }
    fn park_ns(&mut self, ns: u64) -> io::Result<()> {
        let mut state = self.0.borrow_mut();
        state.trace.borrow_mut().push(Effect::Park(ns));
        state.park_error.take().map_or(Ok(()), Err)
    }
}
impl RoomRuntimeHost for RuntimeHost {
    type StartControl = StartControl;
    type FinalControl = FinalControl;
    fn start_wait(&mut self) -> StartControl {
        self.0
            .borrow()
            .trace
            .borrow_mut()
            .push(Effect::StartFactory);
        StartControl(self.0.clone())
    }
    fn final_wait(&mut self, timeout: Duration) -> io::Result<(FinalControl, u64)> {
        let mut state = self.0.borrow_mut();
        state.trace.borrow_mut().push(Effect::FinalFactory(timeout));
        state.timeouts.push(timeout);
        if let Some(error) = state.factory_error.take() {
            return Err(error);
        }
        Ok((FinalControl(self.0.clone()), 1_000_000))
    }
    fn dropped(&mut self, outcome: &RoomOutcome) {
        let mut state = self.0.borrow_mut();
        state.trace.borrow_mut().push(Effect::Dropped);
        state.dropped.push(outcome.clone());
    }
}
struct Ui {
    cancelled: Rc<RefCell<bool>>,
}
impl RoomUiHost for Ui {
    fn attached(&self) -> bool {
        false
    }
    fn cancelled(&self) -> bool {
        *self.cancelled.borrow()
    }
    fn close_controls(&mut self) {}
    fn take_request(&mut self) -> io::Result<Option<RoomUiRequest>> {
        panic!("detached host must not acquire UI requests")
    }
    fn reply(&mut self, _: RoomUiReply) -> io::Result<()> {
        panic!("detached host must not reply")
    }
    fn publish(&mut self, _: Arc<RoomPresentation>) -> Result<(), String> {
        panic!("detached host must not publish")
    }
    fn retry_publication(&mut self) {
        panic!("detached host must not retry")
    }
    fn publish_results(&mut self, _: Arc<RoomResults>) -> Result<(), String> {
        panic!("detached host must not publish results")
    }
}
struct Network {
    trace: Trace,
    snapshot: RoomSnapshot,
    schedule: StartSchedule,
    commit_after: Option<usize>,
    polls: usize,
    replies: VecDeque<RoomReply>,
    commands: Vec<RoomCommand>,
    next: u64,
    clock: VecDeque<i64>,
    after_drain: Option<RoomOutcome>,
    joined_override: Option<RoomOutcome>,
    joined: usize,
}
struct Port(Rc<RefCell<Network>>);
impl RoomNetworkPort for Port {
    fn try_command(&mut self, command: RoomCommand) -> io::Result<u64> {
        let mut state = self.0.borrow_mut();
        state.trace.borrow_mut().push(match &command {
            RoomCommand::Publish {
                final_prefix: true, ..
            } => Effect::Final,
            RoomCommand::Drain => Effect::Drain,
            RoomCommand::Publish {
                final_prefix: false,
                ..
            } => Effect::Progress,
            _ => panic!("no lobby command expected"),
        });
        let id = state.next;
        state.next += 1;
        state.commands.push(command);
        state.replies.push_back(RoomReply { id, result: Ok(()) });
        Ok(id)
    }
    fn poll(&self) -> io::Result<RoomPoll> {
        let mut state = self.0.borrow_mut();
        state.trace.borrow_mut().push(Effect::Poll);
        state.polls += 1;
        if state.commit_after.is_some_and(|count| state.polls >= count) {
            state.snapshot.schedule = Some(state.schedule);
        }
        if state
            .commands
            .iter()
            .any(|command| matches!(command, RoomCommand::Drain))
        {
            if let Some(outcome) = state.after_drain.take() {
                state.snapshot.receipts = outcome.receipts;
                state.snapshot.terminal = Some(outcome);
            }
        }
        Ok(RoomPoll {
            snapshot: state.snapshot.clone(),
            replies: state.replies.drain(..).collect(),
        })
    }
    fn clock_now_ns(&self) -> io::Result<i64> {
        let mut state = self.0.borrow_mut();
        state.trace.borrow_mut().push(Effect::NetworkClock);
        Ok(state.clock.pop_front().unwrap_or(NETWORK_ORIGIN))
    }
    fn request_stop(&self) {
        self.0.borrow().trace.borrow_mut().push(Effect::Stop);
    }
    fn stop(&mut self) -> RoomOutcome {
        let mut state = self.0.borrow_mut();
        state.trace.borrow_mut().push(Effect::Join);
        state.joined += 1;
        let outcome = state
            .joined_override
            .clone()
            .or_else(|| state.snapshot.terminal.clone())
            .unwrap_or_else(|| cancelled_outcome(None));
        state.snapshot.receipts = outcome.receipts;
        state.snapshot.terminal = Some(outcome.clone());
        outcome
    }
}
fn cancelled_outcome(cleanup_error: Option<RoomFailure>) -> RoomOutcome {
    RoomOutcome {
        cancelled: true,
        error: None,
        cleanup_error,
        receipts: RoomReceipts::default(),
        leave_written: false,
    }
}
fn setup_snapshot() -> (RoomSnapshot, StartSchedule) {
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 2, 8, 10_000).unwrap());
    let players = [PlayerId(u32::MAX), PlayerId(7)];
    let ids = [
        registry
            .join("room", b"fixed setup", &players, 0)
            .unwrap()
            .id,
        registry
            .join("room", b"fixed setup", &players, 0)
            .unwrap()
            .id,
    ];
    registry.seal(ids[0], 1).unwrap();
    for id in ids {
        registry.ready(id, 2).unwrap();
    }
    let room = registry.room("room").unwrap();
    let mut filter = ClockFilter::new();
    filter
        .observe(ClockSample::new(1_000, 1_000, 1_000, 1_000).unwrap())
        .unwrap();
    let estimate = filter.estimate().unwrap();
    let policy = StartPolicy::default();
    let mut coordinator = RoomStartCoordinator::new(room, policy).unwrap();
    let mut clients = Vec::new();
    for member in room.members {
        let mut client = StartAgreement::new_at(StartRole::Join, policy, 0).unwrap();
        client.prepare(estimate).unwrap();
        coordinator.prepare(member.id, estimate, 1_000).unwrap();
        let ready = coordinator.next(member.id, 1_000).unwrap().unwrap();
        coordinator.written(member.id, ready, 1_000).unwrap();
        client.receive(ready, 1_000).unwrap();
        let ready = client.next(1_000).unwrap().unwrap();
        client.written(ready, 1_000).unwrap();
        coordinator.receive(member.id, ready, 1_000).unwrap();
        clients.push(client);
    }
    for (member, client) in room.members.iter().zip(&mut clients) {
        let proposal = coordinator.next(member.id, 1_000).unwrap().unwrap();
        coordinator.written(member.id, proposal, 1_000).unwrap();
        client.receive(proposal, 1_000).unwrap();
        let accept = client.next(1_000).unwrap().unwrap();
        client.written(accept, 1_000).unwrap();
        coordinator.receive(member.id, accept, 1_000).unwrap();
    }
    for (member, client) in room.members.iter().zip(&mut clients) {
        let commit = coordinator.next(member.id, 1_000).unwrap().unwrap();
        coordinator.written(member.id, commit, 1_000).unwrap();
        client.receive(commit, 1_000).unwrap();
    }
    let schedule = clients[0].take_schedule().unwrap();
    (
        RoomSnapshot {
            revision: 1,
            participant: Some(ids[0]),
            room: Some(Arc::new(RoomRoster {
                members: room.members.to_vec(),
                phase: room.phase,
                deadline_ns: room.deadline_ns,
            })),
            ..RoomSnapshot::default()
        },
        schedule,
    )
}
type Owner = NativeRoomCompetition<Port, Ui, RuntimeHost>;
fn fixture(
    committed: bool,
) -> (
    Owner,
    Rc<RefCell<Network>>,
    Rc<RefCell<RuntimeState>>,
    Rc<RefCell<bool>>,
) {
    let trace = Rc::new(RefCell::new(vec![]));
    let (mut snapshot, schedule) = setup_snapshot();
    if committed {
        snapshot.schedule = Some(schedule);
    }
    let network = Rc::new(RefCell::new(Network {
        trace: trace.clone(),
        snapshot,
        schedule,
        commit_after: None,
        polls: 0,
        replies: VecDeque::new(),
        commands: vec![],
        next: 9_007_199_254_740_993,
        clock: VecDeque::new(),
        after_drain: None,
        joined_override: None,
        joined: 0,
    }));
    let runtime = Rc::new(RefCell::new(RuntimeState {
        trace,
        start_error: None,
        factory_error: None,
        park_error: None,
        times: [Ok(10), Ok(11), Ok(12), Ok(13), Ok(14)].into(),
        dropped: vec![],
        timeouts: vec![],
    }));
    let cancelled = Rc::new(RefCell::new(false));
    let owner = Owner::new_with_ports(
        Port(network.clone()),
        vec![PlayerId(u32::MAX), PlayerId(7)],
        Duration::from_millis(1),
        Ui {
            cancelled: cancelled.clone(),
        },
        RuntimeHost(runtime.clone()),
    )
    .unwrap();
    (owner, network, runtime, cancelled)
}
fn rows() -> Vec<MemberProgress> {
    [u32::MAX, 7]
        .into_iter()
        .map(|id| MemberProgress {
            player: PlayerId(id),
            progress: Progress {
                song_ns: 604_800_000_000_001,
                hits: 17,
                misses: 3,
                combo: 9,
                max_combo: 15,
            },
        })
        .collect()
}

#[test]
fn actual_pending_startup_uses_injected_wait_control_until_protocol_generated_commit() {
    let (mut owner, network, runtime, _) = fixture(false);
    network.borrow_mut().commit_after = Some(3);
    let trace = runtime.borrow().trace.clone();
    let service_trace = trace.clone();
    assert!(
        owner
            .await_commit(&mut || {
                service_trace.borrow_mut().push(Effect::Service);
                Ok(true)
            })
            .unwrap()
    );
    assert_eq!(
        *trace.borrow(),
        vec![
            Effect::StartFactory,
            Effect::Service,
            Effect::Poll,
            Effect::StartWait(1_000_000),
            Effect::Service,
            Effect::Poll,
            Effect::StartWait(1_000_000),
            Effect::Service,
            Effect::Poll
        ]
    );
    assert_eq!(
        owner.committed_schedule().unwrap(),
        network.borrow().schedule
    );
    assert_eq!(network.borrow().joined, 0);
    assert!(runtime.borrow().timeouts.is_empty());
}

#[derive(Debug)]
struct ServiceRefusal(Arc<u8>);
impl std::fmt::Display for ServiceRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original service refusal")
    }
}
impl std::error::Error for ServiceRefusal {}

#[test]
fn startup_cancel_service_refusal_and_wait_refusal_preserve_original_flow_without_native_waiting() {
    for case in 0..4 {
        let (mut owner, network, runtime, cancelled) = fixture(false);
        let identity = Arc::new(31);
        if case == 0 {
            *cancelled.borrow_mut() = true;
        }
        if case == 3 {
            runtime.borrow_mut().start_error = Some(io::Error::new(
                io::ErrorKind::Interrupted,
                "original virtual wait refusal",
            ));
        }
        let trace = runtime.borrow().trace.clone();
        let service_trace = trace.clone();
        let result = owner.await_commit(&mut || {
            service_trace.borrow_mut().push(Effect::Service);
            if case == 0 {
                panic!("initial cancellation must precede service");
            }
            if case == 1 {
                return Ok(false);
            }
            if case == 2 {
                return Err(Box::new(ServiceRefusal(identity.clone())));
            }
            Ok(true)
        });
        if case <= 1 {
            assert!(matches!(result, Ok(false)));
            assert_eq!(network.borrow().polls, 0);
        } else if case == 2 {
            let error = result.unwrap_err();
            assert!(Arc::ptr_eq(
                &error.downcast_ref::<ServiceRefusal>().unwrap().0,
                &identity
            ));
            assert_eq!(network.borrow().polls, 0);
        } else {
            let error = result.unwrap_err();
            assert_eq!(
                error.downcast_ref::<io::Error>().unwrap().kind(),
                io::ErrorKind::Interrupted
            );
            assert_eq!(network.borrow().polls, 1);
        }
        assert!(trace.borrow().contains(&Effect::Stop));
        assert_eq!(network.borrow().joined, 0);
        assert!(network.borrow().commands.is_empty());
        assert!(runtime.borrow().timeouts.is_empty());
        assert!(owner.committed_schedule().is_err());
    }
}

#[test]
fn actual_natural_finish_queues_original_members_then_drain_and_retains_joined_cleanup_once() {
    for cleanup in [false, true] {
        let (mut owner, network, runtime, _) = fixture(true);
        let members = rows();
        owner.observe(&members).unwrap();
        let terminal = RoomOutcome {
            cancelled: false,
            error: None,
            cleanup_error: None,
            receipts: RoomReceipts {
                local_final_written: true,
                local_final_acknowledged: true,
                progress_complete: true,
                drain_complete: true,
            },
            leave_written: false,
        };
        network.borrow_mut().after_drain = Some(terminal.clone());
        if cleanup {
            let mut joined = terminal.clone();
            joined.cleanup_error = Some(RoomFailure {
                kind: io::ErrorKind::BrokenPipe,
                message: "original joined cleanup failure".into(),
            });
            network.borrow_mut().joined_override = Some(joined);
        }
        let outcome = owner.finish(&members, true);
        assert!(!outcome.cancelled);
        assert_eq!(outcome.error, None);
        assert_eq!(outcome.receipts, terminal.receipts);
        assert_eq!(outcome.cleanup_error.is_some(), cleanup);
        assert_eq!(network.borrow().joined, 1);
        assert_eq!(runtime.borrow().timeouts, [Duration::from_millis(1)]);
        let trace = runtime.borrow().trace.clone();
        let parks: Vec<_> = trace
            .borrow()
            .iter()
            .filter_map(|effect| match effect {
                Effect::Park(ns) => Some(*ns),
                _ => None,
            })
            .collect();
        assert_eq!(parks, [999_989, 999_987]);
        assert_eq!(
            trace
                .borrow()
                .iter()
                .filter(|effect| matches!(effect, Effect::ControlClock))
                .count(),
            5
        );
        let state = network.borrow();
        assert_eq!(state.commands.len(), 3);
        assert!(
            matches!(&state.commands[0],RoomCommand::Publish {members:actual,final_prefix:false} if actual==&members)
        );
        assert!(
            matches!(&state.commands[1],RoomCommand::Publish {members:actual,final_prefix:true} if actual==&members)
        );
        assert!(matches!(&state.commands[2], RoomCommand::Drain));
        drop(state);
        assert_eq!(owner.finish(&members, true), outcome);
        drop(owner);
        assert_eq!(network.borrow().joined, 1);
        assert!(runtime.borrow().dropped.is_empty());
    }
}

#[test]
fn final_factory_controls_and_fixed_deadline_failures_keep_cleanup_and_already_admitted_history() {
    for case in 0..6 {
        let (mut owner, network, runtime, _) = fixture(true);
        let members = rows();
        owner.observe(&members).unwrap();
        let cleanup = RoomFailure {
            kind: io::ErrorKind::BrokenPipe,
            message: "independent cleanup error".into(),
        };
        network.borrow_mut().joined_override = Some(cancelled_outcome(Some(cleanup.clone())));
        match case {
            0 => {
                runtime.borrow_mut().factory_error = Some(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "virtual factory refused",
                ))
            }
            1 => {
                runtime.borrow_mut().times = [Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "virtual preclock refused",
                ))]
                .into()
            }
            2 => {
                runtime.borrow_mut().times = [
                    Ok(10),
                    Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "virtual postclock refused",
                    )),
                ]
                .into()
            }
            3 => {
                runtime.borrow_mut().park_error = Some(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "virtual park refused",
                ))
            }
            4 => runtime.borrow_mut().times = [Ok(1_000_000)].into(),
            _ => network.borrow_mut().clock = [NETWORK_ORIGIN, NETWORK_ORIGIN + 1_000_000].into(),
        }
        let outcome = owner.finish(&members, true);
        assert_eq!(outcome.cleanup_error, Some(cleanup));
        let error = outcome.error.unwrap();
        assert_eq!(
            error.kind,
            if case == 0 {
                io::ErrorKind::PermissionDenied
            } else if case <= 3 {
                io::ErrorKind::Interrupted
            } else {
                io::ErrorKind::TimedOut
            }
        );
        assert_eq!(network.borrow().joined, 1);
        assert_eq!(runtime.borrow().timeouts, [Duration::from_millis(1)]);
        let finals = network
            .borrow()
            .commands
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    RoomCommand::Publish {
                        final_prefix: true,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(finals, usize::from(case == 2 || case == 3));
        drop(owner);
        assert_eq!(network.borrow().joined, 1);
        assert!(runtime.borrow().dropped.is_empty());
    }
}

#[test]
fn explicit_cancelled_finish_skips_control_factory_and_drop_does_not_join_or_report_again() {
    let (mut owner, network, runtime, _) = fixture(false);
    let outcome = owner.finish(&[], false);
    assert!(outcome.cancelled);
    assert!(runtime.borrow().timeouts.is_empty());
    assert_eq!(owner.finish(&[], false), outcome);
    drop(owner);
    assert_eq!(network.borrow().joined, 1);
    assert!(runtime.borrow().dropped.is_empty());
    assert!(network.borrow().commands.is_empty());
}

#[test]
fn implicit_drop_reports_the_actual_joined_outcome_including_both_error_fields_and_receipts() {
    let (owner, network, runtime, _) = fixture(false);
    let joined = RoomOutcome {
        cancelled: true,
        error: Some(RoomFailure {
            kind: io::ErrorKind::ConnectionReset,
            message: "original network error".into(),
        }),
        cleanup_error: Some(RoomFailure {
            kind: io::ErrorKind::BrokenPipe,
            message: "original cleanup error".into(),
        }),
        receipts: RoomReceipts {
            local_final_written: true,
            local_final_acknowledged: false,
            progress_complete: true,
            drain_complete: false,
        },
        leave_written: true,
    };
    network.borrow_mut().joined_override = Some(joined.clone());
    drop(owner);
    assert_eq!(network.borrow().joined, 1);
    assert_eq!(runtime.borrow().dropped, [joined]);
    assert_eq!(
        *runtime.borrow().trace.borrow(),
        vec![Effect::Stop, Effect::Join, Effect::Dropped]
    );
    assert!(runtime.borrow().timeouts.is_empty());
    assert!(network.borrow().commands.is_empty());
}
