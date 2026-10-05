use crate::{
    room_competition::RoomCompetition,
    room_network_model::{
        RoomNetworkPort, RoomSnapshot, RoomRoster, RoomPoll, RoomCommand, RoomReply, RoomOutcome,
        RoomFailure, RoomReceipts,
    },
    room_ui_host::RoomUiHost,
    room_runtime_host::RoomRuntimeHost,
    room_start_wait::{RoomStartWaitControl, RoomStartWaitError},
    final_ack_wait::FinalWaitControl,
    room_presentation::{RoomUiRequest, RoomUiReply, RoomPresentation, RoomResults},
    multiplayer_group_rooms::{GroupRoomRegistry, GroupRoomPolicy},
    multiplayer_clock::{ClockFilter, ClockSample},
    multiplayer_room_start::RoomStartCoordinator,
    multiplayer_start::{StartAgreement, StartPolicy, StartRole, StartSchedule},
    local_players::PlayerId,
};
use std::{cell::RefCell, collections::VecDeque, io, rc::Rc, sync::Arc, time::Duration};

#[derive(Debug, PartialEq, Eq)]
enum Effect {
    StartControl,
    Service,
    Poll,
    Wait(u64),
    Command,
    Clock,
    Stop,
    Join,
    Dropped,
}
type Trace = Rc<RefCell<Vec<Effect>>>;
struct Network {
    trace: Trace,
    snapshot: RoomSnapshot,
    schedule: StartSchedule,
    commit_after: Option<usize>,
    polls: usize,
    poll_error: Option<io::Error>,
    replies: VecDeque<RoomReply>,
    next: u64,
    commands: Vec<RoomCommand>,
    clock: i64,
    joined: RoomOutcome,
    joins: usize,
}
struct Port(Rc<RefCell<Network>>);
impl RoomNetworkPort for Port {
    fn try_command(&mut self, command: RoomCommand) -> io::Result<u64> {
        let mut state = self.0.borrow_mut();
        state.trace.borrow_mut().push(Effect::Command);
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
        if let Some(error) = state.poll_error.take() {
            return Err(error);
        }
        if state.commit_after.is_some_and(|polls| state.polls >= polls) {
            state.snapshot.schedule = Some(state.schedule);
        }
        Ok(RoomPoll {
            snapshot: state.snapshot.clone(),
            replies: state.replies.drain(..).collect(),
        })
    }
    fn clock_now_ns(&self) -> io::Result<i64> {
        let state = self.0.borrow();
        state.trace.borrow_mut().push(Effect::Clock);
        Ok(state.clock)
    }
    fn request_stop(&self) {
        self.0.borrow().trace.borrow_mut().push(Effect::Stop);
    }
    fn stop(&mut self) -> RoomOutcome {
        let mut state = self.0.borrow_mut();
        state.trace.borrow_mut().push(Effect::Join);
        state.joins += 1;
        state.snapshot.terminal = Some(state.joined.clone());
        state.joined.clone()
    }
}
struct Ui(Rc<RefCell<bool>>);
impl RoomUiHost for Ui {
    fn attached(&self) -> bool {
        false
    }
    fn cancelled(&self) -> bool {
        *self.0.borrow()
    }
    fn close_controls(&mut self) {}
    fn take_request(&mut self) -> io::Result<Option<RoomUiRequest>> {
        panic!("detached portable fixture has no UI request")
    }
    fn reply(&mut self, _: RoomUiReply) -> io::Result<()> {
        panic!("detached portable fixture has no UI reply")
    }
    fn publish(&mut self, _: Arc<RoomPresentation>) -> Result<(), String> {
        panic!("detached portable fixture has no publication")
    }
    fn retry_publication(&mut self) {
        panic!("detached portable fixture has no publication retry")
    }
    fn publish_results(&mut self, _: Arc<RoomResults>) -> Result<(), String> {
        panic!("detached portable fixture has no result publication")
    }
}
struct Runtime {
    trace: Trace,
    wait_error: Option<io::Error>,
    dropped: Vec<RoomOutcome>,
}
struct RuntimeHost(Rc<RefCell<Runtime>>);
struct StartControl(Rc<RefCell<Runtime>>);
struct NoFinalControl;
impl RoomStartWaitControl for StartControl {
    type Error = io::Error;
    fn wait_ns(&mut self, ns: u64) -> io::Result<()> {
        let mut runtime = self.0.borrow_mut();
        runtime.trace.borrow_mut().push(Effect::Wait(ns));
        runtime.wait_error.take().map_or(Ok(()), Err)
    }
}
impl FinalWaitControl for NoFinalControl {
    type Error = io::Error;
    fn now_ns(&mut self) -> io::Result<u64> {
        panic!("unplayed/cancelled owners must not sample final controls")
    }
    fn park_ns(&mut self, _: u64) -> io::Result<()> {
        panic!("unplayed/cancelled owners must not park")
    }
}
impl RoomRuntimeHost for RuntimeHost {
    type StartControl = StartControl;
    type FinalControl = NoFinalControl;
    fn start_wait(&mut self) -> StartControl {
        self.0
            .borrow()
            .trace
            .borrow_mut()
            .push(Effect::StartControl);
        StartControl(self.0.clone())
    }
    fn final_wait(&mut self, _: Duration) -> io::Result<(NoFinalControl, u64)> {
        panic!("unplayed/cancelled owners must not construct final controls")
    }
    fn dropped(&mut self, outcome: &RoomOutcome) {
        let mut runtime = self.0.borrow_mut();
        runtime.trace.borrow_mut().push(Effect::Dropped);
        runtime.dropped.push(outcome.clone());
    }
}
fn joined() -> RoomOutcome {
    RoomOutcome {
        cancelled: true,
        error: None,
        cleanup_error: None,
        receipts: RoomReceipts::default(),
        leave_written: false,
    }
}
fn setup() -> (RoomSnapshot, StartSchedule) {
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 2, 8, 10_000).unwrap());
    let players = [PlayerId(u32::MAX), PlayerId(7)];
    let ids = [
        registry
            .join("room", b"original setup", &players, 0)
            .unwrap()
            .id,
        registry
            .join("room", b"original setup", &players, 0)
            .unwrap()
            .id,
    ];
    registry.seal(ids[0], 1).unwrap();
    for id in ids {
        registry.ready(id, 2).unwrap();
    }
    let room = registry.room("room").unwrap();
    let policy = StartPolicy::default();
    let mut filter = ClockFilter::new();
    filter
        .observe(ClockSample::new(1_000, 1_000, 1_000, 1_000).unwrap())
        .unwrap();
    let estimate = filter.estimate().unwrap();
    let mut server = RoomStartCoordinator::new(room, policy).unwrap();
    let mut clients = Vec::new();
    for member in room.members {
        let mut client = StartAgreement::new_at(StartRole::Join, policy, 0).unwrap();
        client.prepare(estimate).unwrap();
        server.prepare(member.id, estimate, 1_000).unwrap();
        let ready = server.next(member.id, 1_000).unwrap().unwrap();
        server.written(member.id, ready, 1_000).unwrap();
        client.receive(ready, 1_000).unwrap();
        let ready = client.next(1_000).unwrap().unwrap();
        client.written(ready, 1_000).unwrap();
        server.receive(member.id, ready, 1_000).unwrap();
        clients.push(client);
    }
    for (member, client) in room.members.iter().zip(&mut clients) {
        let proposal = server.next(member.id, 1_000).unwrap().unwrap();
        server.written(member.id, proposal, 1_000).unwrap();
        client.receive(proposal, 1_000).unwrap();
        let accept = client.next(1_000).unwrap().unwrap();
        client.written(accept, 1_000).unwrap();
        server.receive(member.id, accept, 1_000).unwrap();
    }
    for (member, client) in room.members.iter().zip(&mut clients) {
        let commit = server.next(member.id, 1_000).unwrap().unwrap();
        server.written(member.id, commit, 1_000).unwrap();
        client.receive(commit, 1_000).unwrap();
    }
    let schedule = clients[0].take_schedule().unwrap();
    (
        RoomSnapshot {
            revision: u64::MAX,
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
type Owner = RoomCompetition<Port, Ui, RuntimeHost>;
fn parts(
    committed: bool,
) -> (
    Port,
    Ui,
    RuntimeHost,
    Rc<RefCell<Network>>,
    Rc<RefCell<Runtime>>,
    Rc<RefCell<bool>>,
) {
    let trace = Rc::new(RefCell::new(vec![]));
    let (mut snapshot, schedule) = setup();
    if committed {
        snapshot.schedule = Some(schedule);
    }
    let network = Rc::new(RefCell::new(Network {
        trace: trace.clone(),
        snapshot,
        schedule,
        commit_after: None,
        polls: 0,
        poll_error: None,
        replies: VecDeque::new(),
        next: 9_007_199_254_740_993,
        commands: vec![],
        clock: 9_007_199_254_740_993,
        joined: joined(),
        joins: 0,
    }));
    let runtime = Rc::new(RefCell::new(Runtime {
        trace,
        wait_error: None,
        dropped: vec![],
    }));
    let cancelled = Rc::new(RefCell::new(false));
    (
        Port(network.clone()),
        Ui(cancelled.clone()),
        RuntimeHost(runtime.clone()),
        network,
        runtime,
        cancelled,
    )
}
fn fixture(
    committed: bool,
) -> (
    Owner,
    Rc<RefCell<Network>>,
    Rc<RefCell<Runtime>>,
    Rc<RefCell<bool>>,
) {
    let (port, ui, host, network, runtime, cancelled) = parts(committed);
    (
        Owner::new_with_ports(
            port,
            vec![PlayerId(u32::MAX), PlayerId(7)],
            Duration::from_millis(1),
            ui,
            host,
        )
        .unwrap(),
        network,
        runtime,
        cancelled,
    )
}
// Display is the only service-error trait needed; it is deliberately not Clone, Debug or Error.
struct ServiceFailure(Arc<u8>);
impl std::fmt::Display for ServiceFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original typed service failure")
    }
}

#[test]
fn portable_startup_drives_virtual_pending_waits_and_returns_original_protocol_commit() {
    let (mut owner, network, runtime, _) = fixture(false);
    network.borrow_mut().commit_after = Some(3);
    let trace = runtime.borrow().trace.clone();
    let service_trace = trace.clone();
    let mut service = || -> Result<bool, ServiceFailure> {
        service_trace.borrow_mut().push(Effect::Service);
        Ok(true)
    };
    let callback: &mut dyn FnMut() -> Result<bool, ServiceFailure> = &mut service;
    assert!(matches!(
        owner.await_commit_with_service(callback),
        Ok(true)
    ));
    assert_eq!(
        *trace.borrow(),
        vec![
            Effect::StartControl,
            Effect::Service,
            Effect::Poll,
            Effect::Wait(1_000_000),
            Effect::Service,
            Effect::Poll,
            Effect::Wait(1_000_000),
            Effect::Service,
            Effect::Poll
        ]
    );
    assert_eq!(
        owner.committed_schedule_value().unwrap(),
        network.borrow().schedule
    );
    assert_eq!(owner.players(), [PlayerId(u32::MAX), PlayerId(7)]);
    assert_eq!(owner.snapshot().revision, u64::MAX);
}

#[test]
fn typed_service_failure_retains_original_payload_while_diagnostics_and_stop_are_separate() {
    let (mut owner, network, runtime, _) = fixture(false);
    let identity = Arc::new(47);
    match owner.await_commit_with_service(&mut || Err::<bool, _>(ServiceFailure(identity.clone())))
    {
        Err(RoomStartWaitError::Service(ServiceFailure(actual))) => {
            assert!(Arc::ptr_eq(&actual, &identity))
        }
        _ => panic!("portable startup must return the original typed service payload"),
    }
    assert_eq!(network.borrow().polls, 0);
    assert_eq!(network.borrow().joins, 0);
    assert_eq!(
        owner.network_error().unwrap().message,
        "original typed service failure"
    );
    assert_eq!(owner.network_error().unwrap().kind, io::ErrorKind::Other);
    assert_eq!(
        *runtime.borrow().trace.borrow(),
        vec![Effect::StartControl, Effect::Stop]
    );
    assert_eq!(
        owner.committed_schedule_value().unwrap_err(),
        "native room committed schedule is no longer active"
    );
}

#[test]
fn initial_cancel_service_false_and_wait_refusal_keep_effect_order_without_native_dependencies() {
    for case in 0..3 {
        let (mut owner, network, runtime, cancelled) = fixture(false);
        if case == 0 {
            *cancelled.borrow_mut() = true;
        }
        if case == 2 {
            runtime.borrow_mut().wait_error = Some(io::Error::new(
                io::ErrorKind::Interrupted,
                "virtual startup wait refused",
            ));
        }
        let trace = runtime.borrow().trace.clone();
        let service_trace = trace.clone();
        let result = owner.await_commit_with_service(&mut || -> Result<bool, ServiceFailure> {
            service_trace.borrow_mut().push(Effect::Service);
            if case == 0 {
                panic!("initial cancellation must precede service");
            }
            Ok(case != 1)
        });
        if case <= 1 {
            assert!(matches!(result, Ok(false)));
            assert_eq!(network.borrow().polls, 0);
        } else {
            match result {
                Err(RoomStartWaitError::Control(error)) => {
                    assert_eq!(error.kind(), io::ErrorKind::Interrupted)
                }
                _ => panic!("wait refusal must stay a control error"),
            };
            assert_eq!(network.borrow().polls, 1);
        }
        assert!(trace.borrow().contains(&Effect::Stop));
        assert_eq!(network.borrow().joins, 0);
        assert!(network.borrow().commands.is_empty());
        assert!(owner.committed_schedule_value().is_err());
    }
}

#[test]
fn schedule_guards_refuse_after_actual_leave_terminal_failure_cancel_or_finished_owner() {
    let (mut missing, _, _, _) = fixture(false);
    assert_eq!(
        missing.committed_schedule_value().unwrap_err(),
        "native room committed schedule is missing"
    );
    missing.poll().unwrap();
    assert_eq!(
        missing.committed_schedule_value().unwrap_err(),
        "native room committed schedule is missing"
    );
    for case in 0..5 {
        let (mut owner, network, _, _) = fixture(true);
        owner.poll().unwrap();
        assert_eq!(
            owner.committed_schedule_value().unwrap(),
            network.borrow().schedule
        );
        match case {
            0 => {
                owner.leave().unwrap();
                assert!(owner.committed_schedule_value().is_err());
                owner.poll().unwrap();
            }
            1 => {
                let mut terminal = joined();
                terminal.cancelled = false;
                network.borrow_mut().snapshot.terminal = Some(terminal);
                owner.poll().unwrap();
            }
            2 => {
                network.borrow_mut().poll_error = Some(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    "actual scripted disconnect",
                ));
                assert!(owner.poll().is_err());
            }
            3 => assert!(matches!(
                owner.await_commit_with_service(&mut || Ok::<bool, ServiceFailure>(false)),
                Ok(false)
            )),
            _ => {
                owner.finish(&[], false);
            }
        }
        assert_eq!(
            owner.committed_schedule_value().unwrap_err(),
            "native room committed schedule is no longer active"
        );
    }
}

#[test]
fn constructor_refusal_joins_once_preserving_validation_before_independent_cleanup_error() {
    for cleanup in [false, true] {
        for case in 0..5 {
            let (port, ui, host, network, runtime, _) = parts(false);
            if cleanup {
                network.borrow_mut().joined.cleanup_error = Some(RoomFailure {
                    kind: io::ErrorKind::BrokenPipe,
                    message: "original constructor cleanup".into(),
                });
            }
            let players = match case {
                0 => vec![],
                1 => vec![PlayerId(0)],
                2 => vec![PlayerId(7), PlayerId(7)],
                _ => vec![PlayerId(7)],
            };
            let timeout = if case == 3 {
                Duration::ZERO
            } else if case == 4 {
                Duration::from_millis(120_001)
            } else {
                Duration::from_millis(1)
            };
            let result = Owner::new_with_ports(port, players, timeout, ui, host);
            let error = match result {
                Err(error) => error,
                _ => panic!("invalid constructor must join and refuse"),
            };
            let validation = match case {
                0 => r#"Protocol("group progress requires 1..64 members")"#,
                1 | 2 => r#"Protocol("group progress requires positive unique player identities")"#,
                _ => "native room finish timeout must be 1 ms..120 s",
            };
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert_eq!(
                error.to_string(),
                if cleanup {
                    format!("{validation}; cleanup: original constructor cleanup")
                } else {
                    validation.into()
                }
            );
            assert_eq!(network.borrow().joins, 1);
            assert_eq!(network.borrow().polls, 0);
            assert_eq!(*runtime.borrow().trace.borrow(), vec![Effect::Join]);
            assert!(runtime.borrow().dropped.is_empty());
        }
    }
}

#[test]
fn raw_clock_and_explicit_or_implicit_cleanup_preserve_original_full_width_values_once() {
    for explicit in [false, true] {
        let (mut owner, network, runtime, _) = fixture(false);
        for clock in [i64::MIN, 604_800_000_000_001, i64::MAX] {
            network.borrow_mut().clock = clock;
            assert_eq!(owner.room_clock_ns().unwrap(), clock);
        }
        let actual = RoomOutcome {
            cancelled: true,
            error: Some(RoomFailure {
                kind: io::ErrorKind::ConnectionReset,
                message: "original operational error".into(),
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
        network.borrow_mut().joined = actual.clone();
        if explicit {
            let outcome = owner.finish(&[], false);
            assert_eq!(outcome, actual);
            assert_eq!(owner.finish(&[], false), actual);
        }
        drop(owner);
        assert_eq!(network.borrow().joins, 1);
        if explicit {
            assert!(runtime.borrow().dropped.is_empty());
        } else {
            assert_eq!(runtime.borrow().dropped, [actual]);
        }
        assert!(network.borrow().commands.is_empty());
    }
}
