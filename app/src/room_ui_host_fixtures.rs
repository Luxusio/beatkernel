use super::*;
use crate::{
    multiplayer_group_rooms::{GroupRoomPolicy, GroupRoomRegistry},
    room_presentation::RoomUiRequest,
};
use std::{cell::RefCell, rc::Rc};

struct Network {
    snapshot: NativeRoomSnapshot,
    replies: VecDeque<NativeRoomReply>,
    commands: Vec<(u64, NativeRoomCommand)>,
    polls: usize,
    requested: usize,
    joined: usize,
    next: u64,
    outcome: NativeRoomOutcome,
}
struct Port(Rc<RefCell<Network>>);
impl NativeRoomPort for Port {
    fn try_command(&mut self, command: NativeRoomCommand) -> io::Result<u64> {
        let mut state = self.0.borrow_mut();
        let id = state.next;
        state.next += 1;
        state.commands.push((id, command));
        state
            .replies
            .push_back(NativeRoomReply { id, result: Ok(()) });
        Ok(id)
    }
    fn poll(&self) -> io::Result<NativeRoomPoll> {
        let mut state = self.0.borrow_mut();
        state.polls += 1;
        Ok(NativeRoomPoll {
            snapshot: state.snapshot.clone(),
            replies: state.replies.drain(..).collect(),
        })
    }
    fn clock_now_ns(&self) -> io::Result<i64> {
        panic!("these UI-only operations must not acquire a network clock")
    }
    fn request_stop(&self) {
        self.0.borrow_mut().requested += 1;
    }
    fn stop(&mut self) -> NativeRoomOutcome {
        let mut state = self.0.borrow_mut();
        state.joined += 1;
        state.snapshot.terminal = Some(state.outcome.clone());
        state.outcome.clone()
    }
}
struct Ui {
    attached: bool,
    cancelled: bool,
    closes: usize,
    takes: usize,
    retries: usize,
    requests: VecDeque<io::Result<Option<RoomUiRequest>>>,
    reply_errors: VecDeque<io::ErrorKind>,
    replies: Vec<RoomUiReply>,
    pages: Vec<Arc<RoomPresentation>>,
    results: Vec<Arc<RoomResults>>,
    publish_error: Option<String>,
    result_error: Option<String>,
}
struct Host(Rc<RefCell<Ui>>);
impl RoomUiHost for Host {
    fn attached(&self) -> bool {
        self.0.borrow().attached
    }
    fn cancelled(&self) -> bool {
        self.0.borrow().cancelled
    }
    fn close_controls(&mut self) {
        self.0.borrow_mut().closes += 1;
    }
    fn take_request(&mut self) -> io::Result<Option<RoomUiRequest>> {
        let mut ui = self.0.borrow_mut();
        ui.takes += 1;
        ui.requests.pop_front().unwrap_or(Ok(None))
    }
    fn reply(&mut self, reply: RoomUiReply) -> io::Result<()> {
        let mut ui = self.0.borrow_mut();
        ui.replies.push(reply);
        match ui.reply_errors.pop_front() {
            Some(kind) => Err(io::Error::new(kind, "injected reply refusal")),
            None => Ok(()),
        }
    }
    fn publish(&mut self, page: Arc<RoomPresentation>) -> Result<(), String> {
        let mut ui = self.0.borrow_mut();
        ui.pages.push(page);
        ui.publish_error.take().map_or(Ok(()), Err)
    }
    fn retry_publication(&mut self) {
        self.0.borrow_mut().retries += 1;
    }
    fn publish_results(&mut self, result: Arc<RoomResults>) -> Result<(), String> {
        let mut ui = self.0.borrow_mut();
        ui.results.push(result);
        ui.result_error.take().map_or(Ok(()), Err)
    }
}
fn snapshot(phase: GroupRoomPhase) -> NativeRoomSnapshot {
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 2, 8, 100).unwrap());
    let own = registry
        .join("room", b"original identity", &[PlayerId(7)], 0)
        .unwrap()
        .id;
    let remote = registry
        .join(
            "room",
            b"original identity",
            &(0..8).map(|i| PlayerId(u32::MAX - i)).collect::<Vec<_>>(),
            0,
        )
        .unwrap()
        .id;
    if phase != GroupRoomPhase::Collecting {
        registry.seal(own, 1).unwrap();
    }
    if phase == GroupRoomPhase::Prepared {
        registry.ready(own, 2).unwrap();
        registry.ready(remote, 2).unwrap();
    }
    let room = registry.room("room").unwrap();
    NativeRoomSnapshot {
        revision: 1,
        participant: Some(own),
        room: Some(Arc::new(NativeRoomRoster {
            members: room.members.to_vec(),
            phase: room.phase,
            deadline_ns: room.deadline_ns,
        })),
        ..NativeRoomSnapshot::default()
    }
}
fn fixture(
    phase: GroupRoomPhase,
    attached: bool,
) -> (
    NativeRoomCompetition<Port, Host>,
    Rc<RefCell<Network>>,
    Rc<RefCell<Ui>>,
) {
    let network = Rc::new(RefCell::new(Network {
        snapshot: snapshot(phase),
        replies: VecDeque::new(),
        commands: vec![],
        polls: 0,
        requested: 0,
        joined: 0,
        next: 9_007_199_254_740_993,
        outcome: NativeRoomOutcome {
            cancelled: true,
            error: None,
            cleanup_error: None,
            receipts: Default::default(),
            leave_written: false,
        },
    }));
    let ui = Rc::new(RefCell::new(Ui {
        attached,
        cancelled: false,
        closes: 0,
        takes: 0,
        retries: 0,
        requests: VecDeque::new(),
        reply_errors: VecDeque::new(),
        replies: vec![],
        pages: vec![],
        results: vec![],
        publish_error: None,
        result_error: None,
    }));
    let owner = NativeRoomCompetition::new_with_host(
        Port(network.clone()),
        vec![PlayerId(7)],
        Duration::from_millis(1),
        Host(ui.clone()),
    )
    .unwrap();
    (owner, network, ui)
}

#[test]
fn detached_injected_host_has_no_request_reply_publication_or_retry_effects() {
    let (mut owner, network, ui) = fixture(GroupRoomPhase::Collecting, false);
    owner.poll().unwrap();
    owner.poll().unwrap();
    let ui = ui.borrow();
    assert_eq!(ui.takes, 0);
    assert_eq!(ui.retries, 0);
    assert_eq!(ui.closes, 0);
    assert!(ui.pages.is_empty());
    assert!(ui.replies.is_empty());
    assert!(ui.results.is_empty());
    assert_eq!(network.borrow().polls, 2);
    assert!(network.borrow().commands.is_empty());
    assert_eq!(owner.local_progress(), None);
}

#[test]
fn attached_host_gets_actual_retained_waiting_page_and_retry_without_rebuilding_it() {
    let (mut owner, _, ui) = fixture(GroupRoomPhase::Collecting, true);
    owner.poll().unwrap();
    let first = ui.borrow().pages[0].clone();
    assert_eq!(first.status, RoomStatus::Waiting);
    assert_eq!(first.lobby.members.len(), 2);
    assert!(first.allows(RoomUiAction::Seal));
    assert!(first.allows(RoomUiAction::Leave));
    assert!(!first.allows(RoomUiAction::Ready));
    let retries = ui.borrow().retries;
    owner.poll().unwrap();
    assert_eq!(ui.borrow().pages.len(), 1);
    assert!(ui.borrow().retries > retries);
    assert!(Arc::ptr_eq(owner.ui_presentation.as_ref().unwrap(), &first));
}

#[test]
fn injected_initial_and_poll_cancellation_close_controls_without_commands_or_completion() {
    for startup in [false, true] {
        let (mut owner, network, ui) = fixture(GroupRoomPhase::Collecting, true);
        ui.borrow_mut().cancelled = true;
        if startup {
            assert!(
                !owner
                    .await_commit(&mut || panic!("cancelled startup must not service or wait"))
                    .unwrap()
            );
        } else {
            owner.poll().unwrap();
        }
        assert!(owner.cancelled);
        assert!(ui.borrow().closes > 0);
        assert!(network.borrow().requested > 0);
        assert_eq!(network.borrow().polls, 0);
        assert!(network.borrow().commands.is_empty());
        assert_eq!(owner.local_progress(), None);
        assert_eq!(owner.outcome(), None);
        assert!(ui.borrow().results.is_empty());
        assert!(
            ui.borrow()
                .pages
                .iter()
                .all(|page| page.status == RoomStatus::Closed)
        );
    }
}

#[test]
fn busy_ui_replies_retain_original_ids_and_never_resend_admitted_seal_ready_or_leave() {
    for (phase, action) in [
        (GroupRoomPhase::Collecting, RoomUiAction::Seal),
        (GroupRoomPhase::Frozen, RoomUiAction::Ready),
        (GroupRoomPhase::Collecting, RoomUiAction::Leave),
    ] {
        let (mut owner, network, ui) = fixture(phase, true);
        ui.borrow_mut()
            .requests
            .push_back(Ok(Some(RoomUiRequest { id: 17, action })));
        ui.borrow_mut()
            .reply_errors
            .push_back(io::ErrorKind::WouldBlock);
        owner.poll().unwrap();
        assert_eq!(network.borrow().commands.len(), 1);
        assert!(ui.borrow().replies.is_empty());
        owner.poll().unwrap();
        assert_eq!(ui.borrow().replies.len(), 1);
        assert_eq!(owner.ui_pending.len(), 1);
        owner.poll().unwrap();
        assert_eq!(owner.ui_pending.len(), 0);
        let state = network.borrow();
        assert_eq!(state.commands.len(), 1);
        assert_eq!(state.commands[0].0, 9_007_199_254_740_993);
        assert!(match (&state.commands[0].1, action) {
            (NativeRoomCommand::Seal, RoomUiAction::Seal)
            | (NativeRoomCommand::Ready, RoomUiAction::Ready)
            | (NativeRoomCommand::Leave, RoomUiAction::Leave) => true,
            _ => false,
        });
        assert_eq!(
            ui.borrow().replies,
            [
                RoomUiReply {
                    id: 17,
                    result: Ok(())
                },
                RoomUiReply {
                    id: 17,
                    result: Ok(())
                }
            ]
        );
        assert_eq!(owner.network_error(), None);
    }
}

#[test]
fn local_pages_illegal_actions_and_ui_errors_do_not_create_network_commands_or_scores() {
    let (mut owner, network, ui) = fixture(GroupRoomPhase::Prepared, true);
    ui.borrow_mut().requests.extend([
        Ok(Some(RoomUiRequest {
            id: 31,
            action: RoomUiAction::Page(1),
        })),
        Ok(Some(RoomUiRequest {
            id: 37,
            action: RoomUiAction::Seal,
        })),
    ]);
    owner.poll().unwrap();
    assert_eq!(owner.hud().unwrap().page_index(), 1);
    assert_eq!(
        ui.borrow().replies[0],
        RoomUiReply {
            id: 31,
            result: Ok(())
        }
    );
    assert_eq!(ui.borrow().replies[1].id, 37);
    assert!(ui.borrow().replies[1].result.is_err());
    assert!(network.borrow().commands.is_empty());
    assert_eq!(owner.local_progress(), None);

    for kind in 0..3 {
        let (mut owner, network, ui) = fixture(GroupRoomPhase::Collecting, true);
        match kind {
            0 => ui.borrow_mut().publish_error = Some(format!("{}\n", "p".repeat(600))),
            1 => ui
                .borrow_mut()
                .requests
                .push_back(Err(io::Error::other(format!("{}\n", "t".repeat(600))))),
            _ => {
                ui.borrow_mut().requests.push_back(Ok(Some(RoomUiRequest {
                    id: 41,
                    action: RoomUiAction::Ready,
                })));
                ui.borrow_mut()
                    .reply_errors
                    .extend([io::ErrorKind::WouldBlock, io::ErrorKind::BrokenPipe]);
            }
        }
        owner.poll().unwrap();
        owner.poll().unwrap();
        assert!(owner.ui_error().is_some());
        assert_eq!(owner.network_error(), None);
        assert_eq!(owner.local_progress(), None);
        assert!(network.borrow().commands.is_empty());
        let ui = ui.borrow();
        let error = ui.pages.last().unwrap().error.as_deref().unwrap();
        assert!(error.chars().count() <= 512);
        assert!(!error.chars().any(char::is_control));
        if kind == 2 {
            assert!(ui.closes > 0);
        }
    }
}

#[test]
fn cancelled_finish_publishes_injected_results_after_join_preserving_cleanup_and_missing_receipts()
{
    for refuse_results in [false, true] {
        let (mut owner, network, ui) = fixture(GroupRoomPhase::Prepared, true);
        network.borrow_mut().outcome.cleanup_error = Some(NativeRoomFailure {
            kind: io::ErrorKind::BrokenPipe,
            message: "original cleanup refusal".into(),
        });
        if refuse_results {
            ui.borrow_mut().result_error = Some("results publication refused".into());
        }
        owner.poll().unwrap();
        let outcome = owner.finish(&[], false);
        assert!(outcome.cancelled);
        assert_eq!(outcome.receipts, Default::default());
        assert_eq!(
            outcome.cleanup_error,
            network.borrow().outcome.cleanup_error
        );
        assert_eq!(outcome.error, None);
        assert_eq!(network.borrow().joined, 1);
        assert!(network.borrow().commands.is_empty());
        assert_eq!(ui.borrow().results.len(), 1);
        assert!(ui.borrow().closes > 0);
        let results = ui.borrow().results[0].clone();
        assert!(results.cancelled());
        assert!(
            results
                .error()
                .unwrap()
                .contains("original cleanup refusal")
        );
        assert!(
            results
                .rows()
                .iter()
                .all(|row| row.progress.is_none() && !row.final_prefix)
        );
        let effects = (
            network.borrow().polls,
            ui.borrow().closes,
            ui.borrow().pages.len(),
            ui.borrow().results.len(),
        );
        assert_eq!(owner.finish(&[], false), outcome);
        assert_eq!(
            (
                network.borrow().polls,
                ui.borrow().closes,
                ui.borrow().pages.len(),
                ui.borrow().results.len()
            ),
            effects
        );
        assert_eq!(network.borrow().joined, 1);
        assert_eq!(owner.outcome(), Some(&outcome));
    }
}
