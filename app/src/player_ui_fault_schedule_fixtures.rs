//! Deterministic faults at the actual portable room-controller ports.
//! No native route, socket, GPU, or wall-clock execution is claimed here.
use crate::{
    final_ack_wait::FinalWaitControl,
    local_players::PlayerId,
    multiplayer_group::MemberProgress,
    multiplayer_group_rooms::{GroupRoomPhase, GroupRoomPolicy, GroupRoomRegistry},
    multiplayer_protocol::Progress,
    multiplayer_start::StartSchedule,
    room_competition::RoomCompetition,
    room_network_model::{
        RoomCommand, RoomNetworkPort, RoomOutcome, RoomPoll, RoomReceipts, RoomReply, RoomRoster,
        RoomSnapshot,
    },
    room_presentation::{
        RoomPresentation, RoomResults, RoomStatus, RoomUiAction, RoomUiReply, RoomUiRequest,
    },
    room_runtime_host::RoomRuntimeHost,
    room_start_wait::RoomStartWaitControl,
    room_ui_host::RoomUiHost,
};
use beatkernel::{
    audio::{command_queue, CommandConsumer},
    chart::*,
    input::*,
    interaction::InstantEvaluator,
    judge::*,
    runtime::Runtime,
    time::*,
    transport::{Rate, Transport},
};
use std::{
    cell::RefCell, collections::VecDeque, io, rc::Rc, sync::Arc, time::Duration as WallDuration,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Effect {
    Poll,
    Command(u64, &'static str),
    RefusedCommand(&'static str),
    NetworkClock(i64),
    Stop,
    Join,
    Dropped,
    Close,
    Take,
    Reply(u64, bool),
    ReplyRefused(u64, io::ErrorKind),
    Publish(RoomStatus),
    PublicationRefused,
    Retry,
    Results,
    ResultsRefused,
    FinalFactory,
    ControlClock(u64),
    Park(u64),
}
type Trace = Rc<RefCell<Vec<Effect>>>;
fn label(command: &RoomCommand) -> &'static str {
    match command {
        RoomCommand::Seal => "seal",
        RoomCommand::Ready => "ready",
        RoomCommand::Leave => "leave",
        RoomCommand::Publish {
            final_prefix: false,
            ..
        } => "progress",
        RoomCommand::Publish {
            final_prefix: true, ..
        } => "final",
        RoomCommand::Drain => "drain",
    }
}
fn terminal(complete: bool) -> RoomOutcome {
    RoomOutcome {
        cancelled: !complete,
        error: None,
        cleanup_error: None,
        receipts: RoomReceipts {
            local_final_written: complete,
            local_final_acknowledged: complete,
            progress_complete: complete,
            drain_complete: complete,
        },
        leave_written: false,
    }
}
struct Network {
    trace: Trace,
    snapshot: RoomSnapshot,
    polls: VecDeque<io::Result<RoomPoll>>,
    commands: Vec<(u64, RoomCommand)>,
    refusals: VecDeque<io::ErrorKind>,
    next: u64,
    now: i64,
    joined: RoomOutcome,
    joins: usize,
}
struct Port(Rc<RefCell<Network>>);
impl RoomNetworkPort for Port {
    fn try_command(&mut self, command: RoomCommand) -> io::Result<u64> {
        let mut state = self.0.borrow_mut();
        if let Some(kind) = state.refusals.pop_front() {
            state
                .trace
                .borrow_mut()
                .push(Effect::RefusedCommand(label(&command)));
            return Err(io::Error::new(kind, "scripted command refusal"));
        }
        let id = state.next;
        state.next += 1;
        state
            .trace
            .borrow_mut()
            .push(Effect::Command(id, label(&command)));
        state.commands.push((id, command));
        Ok(id)
    }
    fn poll(&self) -> io::Result<RoomPoll> {
        let mut state = self.0.borrow_mut();
        state.trace.borrow_mut().push(Effect::Poll);
        if let Some(script) = state.polls.pop_front() {
            if let Ok(poll) = &script {
                state.snapshot = poll.snapshot.clone();
            }
            script
        } else {
            Ok(RoomPoll {
                snapshot: state.snapshot.clone(),
                replies: vec![],
            })
        }
    }
    fn clock_now_ns(&self) -> io::Result<i64> {
        let state = self.0.borrow();
        state
            .trace
            .borrow_mut()
            .push(Effect::NetworkClock(state.now));
        Ok(state.now)
    }
    fn request_stop(&self) {
        self.0.borrow().trace.borrow_mut().push(Effect::Stop);
    }
    fn stop(&mut self) -> RoomOutcome {
        let mut state = self.0.borrow_mut();
        state.joins += 1;
        state.trace.borrow_mut().push(Effect::Join);
        state.joined.clone()
    }
}
struct Ui {
    trace: Trace,
    cancelled: bool,
    requests: VecDeque<io::Result<Option<RoomUiRequest>>>,
    reply_refusals: VecDeque<io::ErrorKind>,
    replies: Vec<RoomUiReply>,
    publish_refusals: usize,
    pages: Vec<Arc<RoomPresentation>>,
    result_refusal: bool,
    results: Vec<Arc<RoomResults>>,
    closes: usize,
}
struct Host(Rc<RefCell<Ui>>);
impl RoomUiHost for Host {
    fn attached(&self) -> bool {
        true
    }
    fn cancelled(&self) -> bool {
        self.0.borrow().cancelled
    }
    fn close_controls(&mut self) {
        let mut ui = self.0.borrow_mut();
        ui.closes += 1;
        ui.trace.borrow_mut().push(Effect::Close);
    }
    fn take_request(&mut self) -> io::Result<Option<RoomUiRequest>> {
        let mut ui = self.0.borrow_mut();
        ui.trace.borrow_mut().push(Effect::Take);
        ui.requests.pop_front().unwrap_or(Ok(None))
    }
    fn reply(&mut self, reply: RoomUiReply) -> io::Result<()> {
        let mut ui = self.0.borrow_mut();
        if let Some(kind) = ui.reply_refusals.pop_front() {
            ui.trace
                .borrow_mut()
                .push(Effect::ReplyRefused(reply.id, kind));
            return Err(io::Error::new(kind, "scripted reply refusal"));
        }
        ui.trace
            .borrow_mut()
            .push(Effect::Reply(reply.id, reply.result.is_ok()));
        ui.replies.push(reply);
        Ok(())
    }
    fn publish(&mut self, page: Arc<RoomPresentation>) -> Result<(), String> {
        let mut ui = self.0.borrow_mut();
        if ui.publish_refusals > 0 {
            ui.publish_refusals -= 1;
            ui.trace.borrow_mut().push(Effect::PublicationRefused);
            return Err("scripted presentation refusal".into());
        }
        ui.trace.borrow_mut().push(Effect::Publish(page.status));
        ui.pages.push(page);
        Ok(())
    }
    fn retry_publication(&mut self) {
        self.0.borrow().trace.borrow_mut().push(Effect::Retry);
    }
    fn publish_results(&mut self, result: Arc<RoomResults>) -> Result<(), String> {
        let mut ui = self.0.borrow_mut();
        if ui.result_refusal {
            ui.trace.borrow_mut().push(Effect::ResultsRefused);
            return Err("scripted result refusal".into());
        }
        ui.trace.borrow_mut().push(Effect::Results);
        ui.results.push(result);
        Ok(())
    }
}
struct WaitState {
    trace: Trace,
    now: u64,
    parks: usize,
    cancel_on_park: bool,
    ui: Rc<RefCell<Ui>>,
}
struct WaitHost(Rc<RefCell<WaitState>>);
struct Control(Rc<RefCell<WaitState>>);
impl RoomStartWaitControl for Control {
    type Error = io::Error;
    fn wait_ns(&mut self, _: u64) -> io::Result<()> {
        panic!("startup outside these schedules")
    }
}
impl FinalWaitControl for Control {
    type Error = io::Error;
    fn now_ns(&mut self) -> io::Result<u64> {
        let mut state = self.0.borrow_mut();
        state.now += 1;
        state
            .trace
            .borrow_mut()
            .push(Effect::ControlClock(state.now));
        Ok(state.now)
    }
    fn park_ns(&mut self, ns: u64) -> io::Result<()> {
        let mut state = self.0.borrow_mut();
        state.parks += 1;
        assert!(
            state.parks <= 8,
            "script must terminate within eight pending iterations"
        );
        state.trace.borrow_mut().push(Effect::Park(ns));
        if state.cancel_on_park {
            state.ui.borrow_mut().cancelled = true;
            // Explicit independent control deadline after cancellation: no sleeps,
            // and no missing terminal observation is treated as completion.
            state.now = 1_000_000;
        }
        Ok(())
    }
}
impl RoomRuntimeHost for WaitHost {
    type StartControl = Control;
    type FinalControl = Control;
    fn start_wait(&mut self) -> Control {
        Control(self.0.clone())
    }
    fn final_wait(&mut self, timeout: WallDuration) -> io::Result<(Control, u64)> {
        assert_eq!(timeout, WallDuration::from_millis(1));
        self.0
            .borrow()
            .trace
            .borrow_mut()
            .push(Effect::FinalFactory);
        Ok((Control(self.0.clone()), 1_000_000))
    }
    fn dropped(&mut self, _: &RoomOutcome) {
        self.0.borrow().trace.borrow_mut().push(Effect::Dropped);
    }
}
type Owner = RoomCompetition<Port, Host, WaitHost>;
struct Fixture {
    owner: Owner,
    network: Rc<RefCell<Network>>,
    ui: Rc<RefCell<Ui>>,
    wait: Rc<RefCell<WaitState>>,
    trace: Trace,
}
fn fixture(phase: GroupRoomPhase, committed: bool) -> Fixture {
    let trace = Rc::new(RefCell::new(vec![]));
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 2, 16, 10_000).unwrap());
    let own = registry
        .join("schedule", b"fixed chart", &[PlayerId(7)], 0)
        .unwrap()
        .id;
    let peer = registry
        .join("schedule", b"fixed chart", &[PlayerId(9)], 0)
        .unwrap()
        .id;
    if phase != GroupRoomPhase::Collecting {
        registry.seal(own, 1).unwrap();
    }
    if phase == GroupRoomPhase::Prepared {
        registry.ready(own, 2).unwrap();
        registry.ready(peer, 2).unwrap();
    }
    let room = registry.room("schedule").unwrap();
    let snapshot = RoomSnapshot {
        revision: 1,
        participant: Some(own),
        room: Some(Arc::new(RoomRoster {
            members: room.members.to_vec(),
            phase: room.phase,
            deadline_ns: room.deadline_ns,
        })),
        // Injected network-port commitment evidence; not a test-created native agreement.
        schedule: committed.then_some(StartSchedule {
            target_ns: 0,
            song_target_ns: 0,
            uncertainty_ns: 0,
        }),
        ..Default::default()
    };
    let network = Rc::new(RefCell::new(Network {
        trace: trace.clone(),
        snapshot,
        polls: VecDeque::new(),
        commands: vec![],
        refusals: VecDeque::new(),
        next: 101,
        now: 0,
        joined: terminal(false),
        joins: 0,
    }));
    let ui = Rc::new(RefCell::new(Ui {
        trace: trace.clone(),
        cancelled: false,
        requests: VecDeque::new(),
        reply_refusals: VecDeque::new(),
        replies: vec![],
        publish_refusals: 0,
        pages: vec![],
        result_refusal: false,
        results: vec![],
        closes: 0,
    }));
    let wait = Rc::new(RefCell::new(WaitState {
        trace: trace.clone(),
        now: 0,
        parks: 0,
        cancel_on_park: false,
        ui: ui.clone(),
    }));
    let owner = Owner::new_with_ports(
        Port(network.clone()),
        vec![PlayerId(7)],
        WallDuration::from_millis(1),
        Host(ui.clone()),
        WaitHost(wait.clone()),
    )
    .unwrap();
    Fixture {
        owner,
        network,
        ui,
        wait,
        trace,
    }
}
fn poll_with(f: &Fixture, replies: Vec<RoomReply>, outcome: Option<RoomOutcome>) -> RoomPoll {
    let mut snapshot = f.network.borrow().snapshot.clone();
    if let Some(outcome) = outcome {
        snapshot.receipts = outcome.receipts;
        snapshot.terminal = Some(outcome);
    }
    RoomPoll { snapshot, replies }
}
fn ack(id: u64) -> RoomReply {
    RoomReply { id, result: Ok(()) }
}
struct SameClock;
impl ClockMapper for SameClock {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
struct Gameplay {
    runtime: Runtime,
    _consumer: CommandConsumer,
    hits: u64,
}
impl Gameplay {
    fn new() -> Self {
        let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
        source.objects = [(41, 100), (42, 200)]
            .into_iter()
            .map(|(id, at)| SourceObject {
                id: ObjectId(id),
                start: Beat::new(at).unwrap(),
                end: None,
                interaction: InteractionId(1),
                visual: VisualId(1),
                audio: None,
                metadata: ObjectMetadata::default(),
            })
            .collect();
        let judge = JudgeEngine::new(
            source.compile().unwrap(),
            vec![Rule {
                interaction: InteractionId(1),
                control: GameControlId(1),
                evaluator: Box::new(InstantEvaluator),
            }],
            JudgeProfile::new(
                vec![JudgeWindow {
                    grade: JudgeGrade(7),
                    early: beatkernel::time::Duration::ZERO,
                    late: beatkernel::time::Duration::ZERO,
                }],
                beatkernel::time::Duration::ZERO,
            )
            .unwrap(),
        )
        .unwrap();
        let bindings = BindingMap::from_bindings([1, 2].map(|key| Binding {
            device: DeviceSelector::Any,
            physical: PhysicalControlId::keyboard(key),
            game_control: GameControlId(1),
        }))
        .unwrap();
        let (producer, consumer) = command_queue(4).unwrap();
        Self {
            runtime: Runtime::new(
                ClockDomainId(1),
                ClockDomainId(1),
                Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
                bindings,
                judge,
                producer,
                vec![],
                4,
            )
            .unwrap(),
            _consumer: consumer,
            hits: 0,
        }
    }
    fn hit(&mut self, ordinal: u16) -> Vec<MemberProgress> {
        let at = i64::from(ordinal) * 100;
        let point = ClockPoint {
            domain: ClockDomainId(1),
            timestamp: Timestamp::from_nanos(at),
        };
        let meta = EventMeta::new(DeviceId(33), point, u64::from(ordinal));
        let input = PhysicalInputEvent::Button(ButtonEvent {
            meta,
            control: PhysicalControlId::keyboard(ordinal),
            state: ButtonState::Down,
        });
        let report = self
            .runtime
            .process_input(input.clone(), &SameClock, point)
            .unwrap();
        assert_eq!(report.input, Some(input));
        assert_eq!(report.song_time.as_nanos(), at);
        assert_eq!(report.bound_inputs.len(), 1);
        assert_eq!(report.judge_error, None);
        assert_eq!(
            report.judge_events,
            [JudgeEvent {
                object: ObjectId(40 + u64::from(ordinal)),
                stage: JudgeStage::Instant,
                outcome: JudgeOutcome::Hit {
                    grade: JudgeGrade(7),
                    delta: beatkernel::time::Duration::ZERO
                },
                at: Timestamp::from_nanos(at),
                input: Some(meta),
            }]
        );
        self.hits += report
            .judge_events
            .iter()
            .filter(|event| matches!(event.outcome, JudgeOutcome::Hit { .. }))
            .count() as u64;
        let members = vec![MemberProgress {
            player: PlayerId(7),
            progress: Progress {
                song_ns: report.song_time.as_nanos(),
                hits: self.hits,
                misses: 0,
                combo: self.hits,
                max_combo: self.hits,
            },
        }];
        assert_eq!(
            members[0].progress,
            Progress {
                song_ns: at,
                hits: u64::from(ordinal),
                misses: 0,
                combo: u64::from(ordinal),
                max_combo: u64::from(ordinal)
            }
        );
        members
    }
}

#[test]
fn delayed_ui_command_refused_delivery_and_stale_network_reply_preserve_real_gameplay() {
    let mut f = fixture(GroupRoomPhase::Collecting, false);
    let mut game = Gameplay::new();
    let first = game.hit(1);
    f.ui.borrow_mut().publish_refusals = 1;
    f.ui.borrow_mut()
        .requests
        .push_back(Err(io::Error::from(io::ErrorKind::WouldBlock)));
    f.owner.observe(&first).unwrap();
    assert_eq!(f.owner.ui_error(), Some("scripted presentation refusal"));
    assert!(f.network.borrow().commands.is_empty());
    f.ui.borrow_mut().requests.push_back(Ok(Some(RoomUiRequest {
        id: 77,
        action: RoomUiAction::Seal,
    })));
    f.owner.poll().unwrap();
    assert_eq!(f.network.borrow().commands, [(101, RoomCommand::Seal)]);
    assert!(f.ui.borrow().replies.is_empty());
    // A delayed network receipt belongs to command101, but delivery belongs to UI77.
    let received = poll_with(&f, vec![ack(101)], None);
    f.network.borrow_mut().polls.push_back(Ok(received));
    f.ui.borrow_mut()
        .reply_refusals
        .push_back(io::ErrorKind::WouldBlock);
    let second = game.hit(2);
    f.owner.observe(&second).unwrap();
    assert!(f.ui.borrow().replies.is_empty());
    f.owner.poll().unwrap();
    assert_eq!(
        f.ui.borrow().replies,
        [RoomUiReply {
            id: 77,
            result: Ok(())
        }]
    );
    assert_eq!(f.network.borrow().commands, [(101, RoomCommand::Seal)]);
    assert_eq!(f.owner.local_progress(), Some(second.as_slice()));
    let stale = poll_with(&f, vec![ack(101)], None);
    f.network.borrow_mut().polls.push_back(Ok(stale));
    assert_eq!(
        f.owner.poll().unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        f.owner.network_error().unwrap().message,
        "unknown or duplicate native room reply"
    );
    assert_eq!(f.owner.local_progress(), Some(second.as_slice()));
    assert_eq!(f.ui.borrow().replies.len(), 1);
    let correlated: Vec<_> = f
        .trace
        .borrow()
        .iter()
        .filter(|e| {
            matches!(
                e,
                Effect::Command(..) | Effect::Reply(..) | Effect::ReplyRefused(..)
            )
        })
        .cloned()
        .collect();
    assert_eq!(
        correlated,
        [
            Effect::Command(101, "seal"),
            Effect::ReplyRefused(77, io::ErrorKind::WouldBlock),
            Effect::Reply(77, true)
        ]
    );
    let outcome = f.owner.finish(&second, false);
    assert!(outcome.cancelled);
    assert!(outcome.error.is_some());
    assert_eq!(f.network.borrow().joins, 1);
}

#[test]
fn optional_peer_failure_does_not_swallow_invalid_local_prefix_or_mutate_runtime() {
    let mut f = fixture(GroupRoomPhase::Prepared, true);
    let mut game = Gameplay::new();
    let first = game.hit(1);
    f.network
        .borrow_mut()
        .refusals
        .push_back(io::ErrorKind::WouldBlock);
    f.owner.observe(&first).unwrap();
    assert!(f.owner.network_error().is_none());
    assert!(f.network.borrow().commands.is_empty());
    // The refused publication has no identity or admission marker: same clock retries.
    f.owner.observe(&first).unwrap();
    assert_eq!(
        f.network.borrow().commands,
        [(
            101,
            RoomCommand::Publish {
                members: first.clone(),
                final_prefix: false,
            }
        )]
    );
    let second = game.hit(2);
    f.network.borrow_mut().now = -1;
    f.owner.observe(&second).unwrap(); // Pending publication must not sample that clock.
    assert!(f.owner.network_error().is_none());
    assert_eq!(f.owner.local_progress(), Some(second.as_slice()));
    let runtime_hash = game.runtime.judge_mut().stable_hash();
    let trace_before = f.trace.borrow().clone();
    let mut invalid = second.clone();
    invalid[0].progress.combo = 99;
    assert_eq!(
        f.owner.observe(&invalid).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(*f.trace.borrow(), trace_before); // validation precedes every optional effect
    assert_eq!(f.owner.local_progress(), Some(second.as_slice()));
    assert_eq!(game.runtime.judge_mut().stable_hash(), runtime_hash);
    f.network.borrow_mut().polls.push_back(Err(io::Error::new(
        io::ErrorKind::ConnectionReset,
        "scripted comparison disconnect",
    )));
    f.owner.observe(&second).unwrap();
    assert_eq!(
        f.owner.network_error().unwrap().kind,
        io::ErrorKind::ConnectionReset
    );
    assert_eq!(f.owner.local_progress(), Some(second.as_slice()));
    assert_eq!(game.runtime.judge_mut().stable_hash(), runtime_hash);
    assert_eq!(
        f.ui.borrow().pages.last().unwrap().status,
        RoomStatus::Disconnected
    );
    let outcome = f.owner.finish(&second, true);
    assert!(outcome.cancelled);
    assert_eq!(
        outcome.error.unwrap().message,
        "scripted comparison disconnect"
    );
    assert_eq!(outcome.receipts, RoomReceipts::default());
    assert_eq!(f.network.borrow().commands.len(), 1); // no invented final after failure
    assert!(!f.trace.borrow().contains(&Effect::FinalFactory));
    let commands: Vec<_> = f
        .trace
        .borrow()
        .iter()
        .filter(|e| {
            matches!(
                e,
                Effect::Command(..) | Effect::RefusedCommand(..) | Effect::NetworkClock(..)
            )
        })
        .cloned()
        .collect();
    assert_eq!(
        commands,
        [
            Effect::NetworkClock(0),
            Effect::RefusedCommand("progress"),
            Effect::NetworkClock(0),
            Effect::Command(101, "progress")
        ]
    );
}

#[test]
fn immediate_ui_reply_fatal_refusal_closes_controls_while_contention_retains_only_delivery() {
    for kind in [io::ErrorKind::WouldBlock, io::ErrorKind::BrokenPipe] {
        let mut f = fixture(GroupRoomPhase::Prepared, true);
        let mut game = Gameplay::new();
        let first = game.hit(1);
        f.owner.observe(&first).unwrap();
        let received = poll_with(&f, vec![ack(101)], None);
        f.network.borrow_mut().polls.push_back(Ok(received));
        f.ui.borrow_mut().reply_refusals.push_back(kind);
        f.ui.borrow_mut().requests.extend([
            Ok(Some(RoomUiRequest {
                id: 88,
                action: RoomUiAction::Page(99),
            })),
            Ok(Some(RoomUiRequest {
                id: 89,
                action: RoomUiAction::Page(0),
            })),
        ]);
        f.trace.borrow_mut().clear();
        f.owner.poll().unwrap();
        if kind == io::ErrorKind::BrokenPipe {
            // Fatal immediate delivery must use the same already-existing policy
            // as fatal pending delivery: inspectable error, controls closed, no
            // acquisition or successful delivery of the subsequent action.
            assert_eq!(f.owner.ui_error(), Some("scripted reply refusal"));
            assert_eq!(f.ui.borrow().closes, 1);
            assert!(f.ui.borrow().replies.is_empty());
            assert_eq!(f.ui.borrow().requests.len(), 1);
            assert!(f.trace.borrow().contains(&Effect::Close));
        } else {
            assert!(f.owner.ui_error().is_none());
            assert_eq!(f.ui.borrow().closes, 0);
            // Page0 can complete locally while UI88's refusal is retained.
            assert_eq!(
                f.ui.borrow().replies,
                [RoomUiReply {
                    id: 89,
                    result: Ok(())
                }]
            );
            f.owner.poll().unwrap();
            assert_eq!(
                f.ui.borrow().replies,
                [
                    RoomUiReply {
                        id: 89,
                        result: Ok(())
                    },
                    RoomUiReply {
                        id: 88,
                        result: Err("room action is unavailable in the current owner".into())
                    }
                ]
            );
        }
        assert_eq!(f.owner.local_progress(), Some(first.as_slice()));
        assert_eq!(f.network.borrow().commands.len(), 1);
        assert!(f.owner.network_error().is_none());
        f.owner.finish(&first, false);
    }
}

#[test]
fn final_wait_requires_actual_receipts_and_cancellation_never_publishes_success() {
    // Each case starts fresh actual owners and accepted Runtime inputs. Polls
    // below are injected I/O observations, not an alternative final-wait policy.
    for case in 0..4 {
        let mut f = fixture(GroupRoomPhase::Prepared, true);
        let mut game = Gameplay::new();
        let first = game.hit(1);
        f.owner.observe(&first).unwrap();
        let second = game.hit(2);
        f.owner.observe(&second).unwrap();
        assert_eq!(f.network.borrow().commands.len(), 1); // withheld progress receipt
        let progress_reply = poll_with(&f, vec![ack(101)], None);
        let waiting = poll_with(&f, vec![], None);
        let final_reply = poll_with(&f, vec![ack(102)], None);
        let mut completed = terminal(true);
        if case == 2 {
            completed.receipts.local_final_acknowledged = false;
        }
        let drain_reply = poll_with(&f, vec![ack(103)], Some(completed.clone()));
        f.network.borrow_mut().polls.extend([
            Ok(progress_reply),
            Ok(waiting),
            Ok(final_reply),
            Ok(drain_reply),
        ]);
        f.network.borrow_mut().joined = if case == 3 {
            terminal(false)
        } else {
            completed.clone()
        };
        f.ui.borrow_mut().result_refusal = case == 1;
        f.wait.borrow_mut().cancel_on_park = case == 3;
        f.trace.borrow_mut().clear();
        let outcome = f.owner.finish(&second, true);
        assert_eq!(f.network.borrow().joins, 1);
        assert_eq!(f.owner.local_progress(), Some(second.as_slice()));
        assert_eq!(f.owner.finish(&second, true), outcome); // no repeated join/results
        let commands = f.network.borrow().commands.clone();
        assert_eq!(
            commands[0],
            (
                101,
                RoomCommand::Publish {
                    members: first.clone(),
                    final_prefix: false
                }
            )
        );
        assert_eq!(
            commands[1],
            (
                102,
                RoomCommand::Publish {
                    members: second.clone(),
                    final_prefix: true
                }
            )
        );
        if case != 3 {
            assert_eq!(commands[2], (103, RoomCommand::Drain));
            assert_eq!(commands.len(), 3);
        } else {
            assert_eq!(commands.len(), 2);
        }
        if case <= 1 {
            assert!(!outcome.cancelled);
            assert!(outcome.error.is_none());
            assert_eq!(
                outcome.receipts,
                RoomReceipts {
                    local_final_written: true,
                    local_final_acknowledged: true,
                    progress_complete: true,
                    drain_complete: true
                }
            );
            assert_eq!(f.wait.borrow().parks, 2);
            if case == 0 {
                assert_eq!(f.ui.borrow().results.len(), 1);
                assert!(!f.ui.borrow().results[0].cancelled());
                assert_eq!(f.ui.borrow().results[0].error(), None);
                assert!(f.trace.borrow().contains(&Effect::Results));
            } else {
                assert!(f.ui.borrow().results.is_empty());
                assert_eq!(f.owner.ui_error(), Some("scripted result refusal"));
                assert!(f.trace.borrow().contains(&Effect::ResultsRefused));
                // Result failure is presentation-only. Existing retry_publication
                // retries the current page; it does not rerun joined result creation.
                f.owner.poll().unwrap();
                assert_eq!(f.network.borrow().joins, 1);
                assert_eq!(
                    f.trace
                        .borrow()
                        .iter()
                        .filter(|e| **e == Effect::ResultsRefused)
                        .count(),
                    1
                );
                assert!(f.ui.borrow().results.is_empty());
            }
        } else {
            assert!(outcome.error.is_some());
            if case == 2 {
                assert!(!outcome.cancelled);
                assert_eq!(
                    outcome.error.as_ref().unwrap().kind,
                    io::ErrorKind::InvalidData
                );
                assert_eq!(
                    outcome.error.as_ref().unwrap().message,
                    "native room ended without successful coordinated drain"
                );
                assert!(!outcome.receipts.local_final_acknowledged);
                assert!(f.ui.borrow().results[0]
                    .error()
                    .unwrap()
                    .contains("network:"));
            } else {
                assert!(outcome.cancelled);
                assert_eq!(outcome.receipts, RoomReceipts::default());
                assert_eq!(
                    outcome.error.as_ref().unwrap().kind,
                    io::ErrorKind::TimedOut
                );
                assert!(f.ui.borrow().results[0].cancelled());
                assert_eq!(f.wait.borrow().parks, 1);
            }
        }
        let effects: Vec<_> = f
            .trace
            .borrow()
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Effect::Command(..) | Effect::FinalFactory | Effect::Park(..) | Effect::Join
                )
            })
            .cloned()
            .collect();
        assert_eq!(
            effects,
            if case == 3 {
                vec![
                    Effect::FinalFactory,
                    Effect::Command(102, "final"),
                    Effect::Park(999_998),
                    Effect::Join,
                ]
            } else {
                vec![
                    Effect::FinalFactory,
                    Effect::Command(102, "final"),
                    Effect::Park(999_998),
                    Effect::Command(103, "drain"),
                    Effect::Park(999_996),
                    Effect::Join,
                ]
            }
        );
        assert_eq!(
            f.ui.borrow().pages.last().unwrap().status,
            RoomStatus::Closed
        );
    }
}
