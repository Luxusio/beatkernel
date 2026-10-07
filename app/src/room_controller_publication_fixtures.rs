//! Deferred actual portable controller admission with injected snapshot/clock edges.
use super::*;
use crate::{
    room_network_model::{
        RoomNetworkPort, RoomOutcome, RoomPoll, RoomCommand, RoomReply, RoomSnapshot, RoomReceipts,
    },
    room_ui_host::RoomUiHost,
    room_runtime_host::RoomRuntimeHost,
    room_start_wait::RoomStartWaitControl,
    final_ack_wait::FinalWaitControl,
    room_final_wait::RoomFinalPort,
    room_presentation::{RoomUiRequest, RoomUiReply, RoomPresentation, RoomResults},
    multiplayer_protocol::Progress,
};
use std::{rc::Rc, cell::RefCell};
#[derive(Default)]
struct State {
    now: i64,
    clocks: usize,
    next: u64,
    blocked: bool,
    acknowledge: bool,
    commands: Vec<RoomCommand>,
    replies: Vec<RoomReply>,
    stopped: usize,
}
struct Port(Rc<RefCell<State>>);
fn outcome() -> RoomOutcome {
    RoomOutcome {
        cancelled: true,
        error: None,
        cleanup_error: None,
        receipts: RoomReceipts::default(),
        leave_written: false,
    }
}
impl RoomNetworkPort for Port {
    fn try_command(&mut self, command: RoomCommand) -> io::Result<u64> {
        let mut state = self.0.borrow_mut();
        if state.blocked {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }
        state.next += 1;
        let id = state.next;
        state.commands.push(command);
        if state.acknowledge {
            state.replies.push(RoomReply { id, result: Ok(()) });
        }
        Ok(id)
    }
    fn poll(&self) -> io::Result<RoomPoll> {
        let mut state = self.0.borrow_mut();
        let replies = std::mem::take(&mut state.replies);
        // This is injected network-boundary evidence, not a forged core Commit.
        Ok(RoomPoll {
            snapshot: RoomSnapshot {
                schedule: Some(StartSchedule {
                    target_ns: 0,
                    song_target_ns: 0,
                    uncertainty_ns: 0,
                }),
                ..Default::default()
            },
            replies,
        })
    }
    fn clock_now_ns(&self) -> io::Result<i64> {
        let mut state = self.0.borrow_mut();
        state.clocks += 1;
        Ok(state.now)
    }
    fn request_stop(&self) {
        self.0.borrow_mut().stopped += 1;
    }
    fn stop(&mut self) -> RoomOutcome {
        outcome()
    }
}
struct Ui;
impl RoomUiHost for Ui {
    fn attached(&self) -> bool {
        false
    }
    fn cancelled(&self) -> bool {
        false
    }
    fn close_controls(&mut self) {}
    fn take_request(&mut self) -> io::Result<Option<RoomUiRequest>> {
        panic!("unattached UI request")
    }
    fn reply(&mut self, _: RoomUiReply) -> io::Result<()> {
        panic!("unattached UI reply")
    }
    fn publish(&mut self, _: Arc<RoomPresentation>) -> Result<(), String> {
        panic!("unattached UI publish")
    }
    fn retry_publication(&mut self) {}
    fn publish_results(&mut self, _: Arc<RoomResults>) -> Result<(), String> {
        Ok(())
    }
}

struct AttachedUi(Rc<RefCell<Vec<crate::room_presentation::RoomStatus>>>);
impl RoomUiHost for AttachedUi {
    fn attached(&self) -> bool {
        true
    }
    fn cancelled(&self) -> bool {
        false
    }
    fn close_controls(&mut self) {}
    fn take_request(&mut self) -> io::Result<Option<RoomUiRequest>> {
        Ok(None)
    }
    fn reply(&mut self, _: RoomUiReply) -> io::Result<()> {
        Ok(())
    }
    fn publish(&mut self, page: Arc<RoomPresentation>) -> Result<(), String> {
        self.0.borrow_mut().push(page.status);
        Ok(())
    }
    fn retry_publication(&mut self) {}
    fn publish_results(&mut self, _: Arc<RoomResults>) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn pending_progress_keeps_connected_ui_and_does_not_sample_or_admit_again() {
    let state = Rc::new(RefCell::new(State::default()));
    let pages = Rc::new(RefCell::new(Vec::new()));
    let mut owner = RoomCompetition::new_with_ports(
        Port(state.clone()),
        vec![PlayerId(u32::MAX), PlayerId(7)],
        Duration::from_secs(1),
        AttachedUi(pages.clone()),
        Virtual,
    )
    .unwrap();
    owner.observe(&rows(0)).unwrap();
    assert!(owner.pending(CommandKind::Progress));
    let clocks = state.borrow().clocks;
    state.borrow_mut().now = -1;
    owner.observe(&rows(1)).unwrap();
    assert_eq!(state.borrow().clocks, clocks);
    assert_eq!(state.borrow().commands.len(), 1);
    assert_eq!(owner.local_progress(), Some(rows(1).as_slice()));
    assert!(owner.failure.is_none());
    assert_eq!(
        pages.borrow().last(),
        Some(&crate::room_presentation::RoomStatus::Connected)
    );
    assert!(
        !pages
            .borrow()
            .contains(&crate::room_presentation::RoomStatus::Closed)
    );
}
struct Virtual;
impl RoomStartWaitControl for Virtual {
    type Error = io::Error;
    fn wait_ns(&mut self, _: u64) -> io::Result<()> {
        panic!("no startup wait")
    }
}
impl FinalWaitControl for Virtual {
    type Error = io::Error;
    fn now_ns(&mut self) -> io::Result<u64> {
        panic!("no final clock")
    }
    fn park_ns(&mut self, _: u64) -> io::Result<()> {
        panic!("no final park")
    }
}
impl RoomRuntimeHost for Virtual {
    type StartControl = Virtual;
    type FinalControl = Virtual;
    fn start_wait(&mut self) -> Virtual {
        Virtual
    }
    fn final_wait(&mut self, _: Duration) -> io::Result<(Virtual, u64)> {
        panic!("no blocking final wait")
    }
    fn dropped(&mut self, _: &RoomOutcome) {}
}
fn owner() -> (RoomCompetition<Port, Ui, Virtual>, Rc<RefCell<State>>) {
    let state = Rc::new(RefCell::new(State {
        acknowledge: true,
        ..Default::default()
    }));
    (
        RoomCompetition::new_with_ports(
            Port(state.clone()),
            vec![PlayerId(u32::MAX), PlayerId(7)],
            Duration::from_secs(1),
            Ui,
            Virtual,
        )
        .unwrap(),
        state,
    )
}
fn rows(song: i64) -> Vec<MemberProgress> {
    [PlayerId(u32::MAX), PlayerId(7)]
        .into_iter()
        .map(|player| MemberProgress {
            player,
            progress: Progress {
                song_ns: song,
                hits: 1,
                misses: 0,
                combo: 1,
                max_combo: 1,
            },
        })
        .collect()
}
#[test]
fn retained_local_prefix_advances_under_suppression_and_exact_boundary_commits_once() {
    let (mut owner, state) = owner();
    owner.observe(&rows(0)).unwrap();
    assert_eq!(state.borrow().commands.len(), 1);
    state.borrow_mut().now = 49_999_999;
    owner.observe(&rows(1)).unwrap();
    assert_eq!(owner.local_progress(), Some(rows(1).as_slice()));
    assert_eq!(state.borrow().commands.len(), 1);
    assert_eq!(owner.publication.last_published(), Some(0));
    state.borrow_mut().now = 50_000_000;
    owner.observe(&rows(2)).unwrap();
    assert_eq!(state.borrow().commands.len(), 2);
    assert_eq!(owner.publication.last_published(), Some(50_000_000));
    assert!(state.borrow().commands.iter().all(|command| matches!(
        command,
        RoomCommand::Publish {
            final_prefix: false,
            ..
        }
    )));
}
#[test]
fn would_block_does_not_commit_and_pending_command_keeps_original_pre_clock_gate() {
    let (mut owner, state) = owner();
    state.borrow_mut().blocked = true;
    owner.observe(&rows(0)).unwrap();
    assert_eq!(owner.publication.last_published(), None);
    assert!(state.borrow().commands.is_empty());
    state.borrow_mut().blocked = false;
    owner.observe(&rows(1)).unwrap();
    assert_eq!(owner.publication.last_published(), Some(0));
    state.borrow_mut().now = 50_000_000;
    state.borrow_mut().acknowledge = false;
    owner.observe(&rows(2)).unwrap();
    let clocks = state.borrow().clocks;
    state.borrow_mut().now = -1;
    owner.observe(&rows(3)).unwrap();
    assert_eq!(state.borrow().clocks, clocks);
    assert_eq!(owner.local_progress(), Some(rows(3).as_slice()));
    assert!(owner.failure.is_none());
}
#[test]
fn eligible_suppressed_regression_disables_network_but_preserves_latest_local_prefix_and_forced_final_admission()
 {
    let (mut owner, state) = owner();
    owner.observe(&rows(0)).unwrap();
    state.borrow_mut().now = 49_999_999;
    owner.observe(&rows(1)).unwrap();
    state.borrow_mut().now = 49_999_998;
    owner.observe(&rows(2)).unwrap();
    assert!(owner.failure.is_some());
    assert_eq!(owner.local_progress(), Some(rows(2).as_slice()));
    let (mut owner, state) = self::owner();
    owner.observe(&rows(0)).unwrap();
    state.borrow_mut().now = 1;
    owner.observe(&rows(1)).unwrap();
    let members = rows(1);
    let clocks = state.borrow().clocks;
    let mut final_port = RoomFinalAdapter {
        owner: &mut owner,
        members: &members,
    };
    assert!(matches!(
        final_port.queue_final().unwrap(),
        crate::room_final_wait::RoomFinalAdmission::Accepted
    ));
    assert_eq!(state.borrow().clocks, clocks);
    assert_eq!(owner.publication.last_published(), Some(1));
    assert!(matches!(
        state.borrow().commands.last(),
        Some(RoomCommand::Publish {
            final_prefix: true,
            ..
        })
    ));
    assert!(!owner.final_accepted); // Queue admission is neither actor ACK nor completion.
}
