//! Real actor event ordering. All controls are scoped to one server future.
//! Run ignored cases only in an owned, externally bounded test process with
//! fresh strict PKI supplied in its environment. No credentials are generated here.

use super::*;
use crate::{
    competition_live::CompetitionOptions,
    multiplayer::Progress,
    multiplayer_group::MemberProgress,
    multiplayer_room_io::RoomPlayIo,
    multiplayer_start::{StartRole, StartSchedule},
    multiplayer_webtransport_client::{
        WebTransportEndpoint, WebTransportOptions, WebTransportStream,
    },
    native_cohort_setup::{prepare_audio_cohort_with_policy, CohortPreparation},
    native_group_competition::NativeGroupCompetition,
    native_judge::prepare_policy_competition_identity,
    play_policy::{ClassifiedWindow, ResolvedPlayPolicy},
    PreparedBms,
};
use beatkernel::{
    audio::{AudioFormat, PcmLimits, SampleBank},
    input::DeviceId,
    judge::{JudgeGrade, JudgeWindow},
    time::{ClockDomainId, Duration as SongDuration, Timestamp},
};
use beatkernel_bms::{BmsGaugeKind, BmsJudgment};
use std::{
    cell::RefCell,
    future::Future,
    net::UdpSocket,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{atomic::AtomicBool, mpsc as completion_channel},
    thread::{self, JoinHandle},
    time::Instant as LocalInstant,
};
use tokio::sync::oneshot;

const WAIT: Duration = Duration::from_secs(12);
const CLOSE: Duration = Duration::from_secs(2);
const PREROLL: [i64; 3] = [10_000_000, 25_000_000, 40_000_000];
type Terminal = (ParticipantId, io::Result<()>, Resource);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Failure {
    kind: io::ErrorKind,
    text: String,
}
impl Failure {
    fn capture(error: &io::Error) -> Self {
        Self {
            kind: error.kind(),
            text: error.to_string(),
        }
    }
    fn error(&self) -> io::Error {
        io::Error::new(self.kind, self.text.clone())
    }
}
fn check(condition: bool, message: &str) -> io::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(invalid(message))
    }
}
fn public_error(error: impl fmt::Display) -> io::Error {
    invalid(error)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    identity: Vec<u8>,
    members: Vec<GroupRoomMember>,
    phase: GroupRoomPhase,
    deadline_ns: Option<i64>,
    resources: Vec<ParticipantId>,
    frames: Vec<ParticipantId>,
    prepared: bool,
}
fn snapshot(
    key: &str,
    registry: &GroupRoomRegistry,
    resources: &BTreeMap<ParticipantId, PeerResource>,
    frames: &Frames,
    rooms: &BTreeMap<String, PreparedRoom>,
) -> io::Result<Snapshot> {
    let room = registry
        .room(key)
        .ok_or_else(|| invalid("observed room is absent"))?;
    let scoped: Vec<_> = resources
        .iter()
        .filter(|(_, peer)| peer.key == key)
        .map(|(&id, _)| id)
        .collect();
    // Each actor fixture admits only this exact key. Include all actual frame
    // IDs so even a dangling old frame with no Resource cannot be hidden.
    let frame_ids = frames.keys().copied().collect();
    Ok(Snapshot {
        identity: room.identity.to_vec(),
        members: room.members.to_vec(),
        phase: room.phase,
        deadline_ns: room.deadline_ns,
        resources: scoped,
        frames: frame_ids,
        prepared: rooms.contains_key(key),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WriteToken {
    id: ParticipantId,
    write_id: u64,
    target_ns: i64,
    admitted_ns: i64,
    completed_ns: i64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct CommitState {
    token: WriteToken,
    in_flight: Option<(u64, ControlReceipt, i64)>,
    others: Vec<(ParticipantId, Option<i64>, bool)>,
    committed: bool,
    active: bool,
}
fn commit_state(
    state: &ControlState,
    rooms: &BTreeMap<String, PreparedRoom>,
) -> io::Result<CommitState> {
    let token = state
        .token
        .ok_or_else(|| invalid("held write token absent"))?;
    let room = rooms
        .get(&state.key)
        .ok_or_else(|| invalid("held prepared room absent"))?;
    let peer = room
        .peers
        .iter()
        .find(|peer| peer.id == token.id)
        .ok_or_else(|| invalid("held peer absent"))?;
    Ok(CommitState {
        token,
        in_flight: peer.in_flight,
        others: room
            .peers
            .iter()
            .filter(|peer| peer.id != token.id)
            .map(|peer| (peer.id, peer.commit_admitted, peer.in_flight.is_none()))
            .collect(),
        committed: room.coordinator.committed(),
        active: room.relay.active(),
    })
}
fn held_commit(state: &CommitState) -> bool {
    state.in_flight
        == Some((
            state.token.write_id,
            ControlReceipt::Start(StartMessage::Commit(state.token.target_ns)),
            state.token.admitted_ns,
        ))
        && !state.committed
        && !state.active
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct TerminalFingerprint {
    id: ParticipantId,
    result: Result<(), Failure>,
    stream_present: bool,
    permits: usize,
}
fn terminal_fingerprint(terminal: &Terminal) -> TerminalFingerprint {
    TerminalFingerprint {
        id: terminal.0,
        result: terminal.1.as_ref().map(|_| ()).map_err(Failure::capture),
        stream_present: terminal.2.stream.is_some(),
        permits: terminal.2._permit.num_permits(),
    }
}

enum Notice {
    Started {
        bind: SocketAddr,
        commands: mpsc::Sender<Command>,
    },
    WrittenHeld {
        original: Command,
        state: CommitState,
    },
    OtherCommitReceiptsApplied(CommitState),
    WrittenApplied {
        token: WriteToken,
        ordinal: u64,
        state: CommitState,
    },
    TerminalHeld {
        fingerprint: TerminalFingerprint,
        release: oneshot::Sender<()>,
    },
    ReplacementAdmitted(Snapshot),
    WrittenSkipped {
        token: WriteToken,
        ordinal: u64,
        replacement: Snapshot,
    },
    TerminalSkipped {
        fingerprint: TerminalFingerprint,
        ordinal: u64,
        replacement: Snapshot,
    },
}
struct ControlState {
    key: String,
    old_identity: Vec<u8>,
    notices: mpsc::Sender<Notice>,
    stop: Option<oneshot::Receiver<()>>,
    token: Option<WriteToken>,
    barrier_sent: bool,
    replayed: bool,
    applied: bool,
    terminal: Option<TerminalFingerprint>,
    terminal_passed: bool,
    baseline: Option<Snapshot>,
    ordinal: u64,
}
impl ControlState {
    fn send(&self, notice: Notice) -> io::Result<()> {
        self.notices
            .try_send(notice)
            .map_err(|error| invalid(format!("actor notice: {error}")))
    }
    fn next_ordinal(&mut self) -> io::Result<u64> {
        self.ordinal = self
            .ordinal
            .checked_add(1)
            .ok_or_else(|| invalid("observer ordinal overflow"))?;
        Ok(self.ordinal)
    }
    fn other_receipts(&mut self, rooms: &BTreeMap<String, PreparedRoom>) -> io::Result<()> {
        if self.token.is_none() || self.replayed || self.barrier_sent {
            return Ok(());
        }
        let observed = commit_state(self, rooms)?;
        if held_commit(&observed)
            && observed.others.len() == 2
            && observed
                .others
                .iter()
                .all(|(_, admitted, cleared)| admitted.is_some() && *cleared)
        {
            self.send(Notice::OtherCommitReceiptsApplied(observed))?;
            self.barrier_sent = true;
        }
        Ok(())
    }
}
tokio::task_local! { static CONTROL: RefCell<ControlState>; }

pub(super) fn started(bind: SocketAddr, commands: mpsc::Sender<Command>) -> io::Result<()> {
    CONTROL
        .try_with(|cell| cell.borrow().send(Notice::Started { bind, commands }))
        .unwrap_or(Ok(()))
}
pub(super) async fn shutdown<F: Future<Output = io::Result<()>>>(original: F) -> io::Result<()> {
    let stopped = CONTROL
        .try_with(|cell| cell.borrow_mut().stop.take())
        .ok()
        .flatten();
    if let Some(stopped) = stopped {
        let _ = stopped.await;
        Ok(())
    } else {
        original.await
    }
}
pub(super) fn intercept_command(
    command: Command,
    registry: &GroupRoomRegistry,
    _resources: &BTreeMap<ParticipantId, PeerResource>,
    _frames: &Frames,
    rooms: &BTreeMap<String, PreparedRoom>,
) -> io::Result<Option<Command>> {
    let mut original = Some(command);
    CONTROL
        .try_with(|cell| -> io::Result<()> {
            let mut state = cell.borrow_mut();
            let command = original
                .as_ref()
                .ok_or_else(|| invalid("original command absent"))?;
            let PeerEvent::Written {
                write_id,
                completed_ns,
            } = &command.event
            else {
                return Ok(());
            };
            if let Some(token) = state.token {
                if command.id == token.id && *write_id == token.write_id {
                    check(
                        *completed_ns == token.completed_ns && !state.replayed,
                        "replay token changed or repeated",
                    )?;
                    state.replayed = true;
                }
                return Ok(());
            }
            let Some(roster) = registry.room(&state.key) else {
                return Ok(());
            };
            if roster.identity != state.old_identity
                || roster.members.len() != 3
                || roster.members[1].id != command.id
            {
                return Ok(());
            }
            let Some(room) = rooms.get(&state.key) else {
                return Ok(());
            };
            let peer = room
                .peers
                .iter()
                .find(|peer| peer.id == command.id)
                .ok_or_else(|| invalid("target prepared peer absent"))?;
            let Some((
                expected,
                ControlReceipt::Start(StartMessage::Commit(target_ns)),
                admitted_ns,
            )) = peer.in_flight
            else {
                return Ok(());
            };
            check(
                expected == *write_id && *completed_ns >= admitted_ns && target_ns > 0,
                "actual Commit Written differs from original in-flight receipt",
            )?;
            state.token = Some(WriteToken {
                id: command.id,
                write_id: *write_id,
                target_ns,
                admitted_ns,
                completed_ns: *completed_ns,
            });
            let observed = commit_state(&state, rooms)?;
            check(
                held_commit(&observed),
                "Commit hold did not preserve uncommitted original state",
            )?;
            state.send(Notice::WrittenHeld {
                original: original
                    .take()
                    .ok_or_else(|| invalid("write already moved"))?,
                state: observed,
            })?;
            // This check also covers the case where all other writers finished
            // before this receipt reached requests.recv: no later command is assumed.
            state.other_receipts(rooms)
        })
        .unwrap_or(Ok(()))?;
    Ok(original)
}
pub(super) fn skipped_command(
    command: &Command,
    registry: &GroupRoomRegistry,
    resources: &BTreeMap<ParticipantId, PeerResource>,
    frames: &Frames,
    rooms: &BTreeMap<String, PreparedRoom>,
) -> io::Result<()> {
    CONTROL
        .try_with(|cell| -> io::Result<()> {
            let mut state = cell.borrow_mut();
            let Some(token) = state.token else {
                return Ok(());
            };
            if command.id != token.id {
                return Ok(());
            }
            let PeerEvent::Written {
                write_id,
                completed_ns,
            } = &command.event
            else {
                return Ok(());
            };
            check(
                state.replayed
                    && !state.applied
                    && *write_id == token.write_id
                    && *completed_ns == token.completed_ns,
                "skipped receipt token changed",
            )?;
            let replacement = snapshot(&state.key, registry, resources, frames, rooms)?;
            check(
                state.baseline.as_ref() == Some(&replacement),
                "Written changed independent replacement baseline",
            )?;
            let ordinal = state.next_ordinal()?;
            state.applied = true;
            state.send(Notice::WrittenSkipped {
                token,
                ordinal,
                replacement,
            })
        })
        .unwrap_or(Ok(()))
}
pub(super) fn after_command(
    id: ParticipantId,
    _registry: &GroupRoomRegistry,
    _resources: &BTreeMap<ParticipantId, PeerResource>,
    _frames: &Frames,
    rooms: &BTreeMap<String, PreparedRoom>,
) -> io::Result<()> {
    CONTROL
        .try_with(|cell| -> io::Result<()> {
            let mut state = cell.borrow_mut();
            // Cancellation intentionally removes this room while the receipt is
            // held. Its positive other-receipt barrier must already have occurred.
            if !state.barrier_sent && rooms.contains_key(&state.key) {
                state.other_receipts(rooms)?;
            }
            if state.replayed
                && !state.applied
                && state.token.is_some_and(|token| token.id == id)
                && rooms.contains_key(&state.key)
            {
                let observed = commit_state(&state, rooms)?;
                check(
                    observed.in_flight.is_none() && observed.committed && observed.active,
                    "original replay failed to commit and activate room",
                )?;
                let ordinal = state.next_ordinal()?;
                state.applied = true;
                state.send(Notice::WrittenApplied {
                    token: observed.token,
                    ordinal,
                    state: observed,
                })?;
            }
            Ok(())
        })
        .unwrap_or(Ok(()))
}
pub(super) fn after_admission(
    registry: &GroupRoomRegistry,
    resources: &BTreeMap<ParticipantId, PeerResource>,
    frames: &Frames,
    rooms: &BTreeMap<String, PreparedRoom>,
) -> io::Result<()> {
    CONTROL
        .try_with(|cell| -> io::Result<()> {
            let mut state = cell.borrow_mut();
            if state.baseline.is_some() || state.token.is_none() {
                return Ok(());
            }
            let Some(room) = registry.room(&state.key) else {
                return Ok(());
            };
            if room.identity == state.old_identity || room.members.len() != 3 {
                return Ok(());
            }
            let observed = snapshot(&state.key, registry, resources, frames, rooms)?;
            check(
                observed.phase == GroupRoomPhase::Collecting && !observed.prepared,
                "replacement admission was not independently Collecting",
            )?;
            state.send(Notice::ReplacementAdmitted(observed.clone()))?;
            state.baseline = Some(observed);
            Ok(())
        })
        .unwrap_or(Ok(()))
}
pub(super) fn intercept_terminal(
    terminal: Terminal,
    peers: &mut JoinSet<Terminal>,
) -> io::Result<Option<Terminal>> {
    let mut original = Some(terminal);
    CONTROL
        .try_with(|cell| -> io::Result<()> {
            let mut state = cell.borrow_mut();
            let terminal = original
                .as_ref()
                .ok_or_else(|| invalid("original terminal absent"))?;
            if !state.token.is_some_and(|token| token.id == terminal.0) {
                return Ok(());
            }
            // The normal case has already applied its receipt. Only the two stale
            // cases retain an old terminal while a different room is admitted.
            if state.applied && state.baseline.is_none() {
                return Ok(());
            }
            let fingerprint = terminal_fingerprint(terminal);
            if let Some(expected) = &state.terminal {
                check(
                    expected == &fingerprint && !state.terminal_passed,
                    "terminal result/resource fingerprint changed",
                )?;
                state.terminal_passed = true;
                return Ok(());
            }
            let (release, released) = oneshot::channel();
            let terminal = original
                .take()
                .ok_or_else(|| invalid("terminal already moved"))?;
            // Retain the ORIGINAL whole tuple/Resource/permit in the SAME JoinSet.
            // Dropping the fixture gate releases it; abort_all also disposes it.
            peers.spawn(async move {
                let _ = released.await;
                terminal
            });
            state.terminal = Some(fingerprint.clone());
            state.send(Notice::TerminalHeld {
                fingerprint,
                release,
            })
        })
        .unwrap_or(Ok(()))?;
    Ok(original)
}
pub(super) fn after_terminal(
    id: ParticipantId,
    result: &io::Result<()>,
    had_resource: bool,
    registry: &GroupRoomRegistry,
    resources: &BTreeMap<ParticipantId, PeerResource>,
    frames: &Frames,
    rooms: &BTreeMap<String, PreparedRoom>,
) -> io::Result<()> {
    CONTROL
        .try_with(|cell| -> io::Result<()> {
            let mut state = cell.borrow_mut();
            let Some(fingerprint) = state
                .terminal
                .clone()
                .filter(|fingerprint| fingerprint.id == id)
            else {
                return Ok(());
            };
            check(
                state.terminal_passed
                    && !had_resource
                    && result.as_ref().map(|_| ()).map_err(Failure::capture) == fingerprint.result,
                "terminal did not execute original missing-resource path with exact result",
            )?;
            let replacement = snapshot(&state.key, registry, resources, frames, rooms)?;
            check(
                state.baseline.as_ref() == Some(&replacement),
                "terminal changed independent replacement baseline",
            )?;
            let ordinal = state.next_ordinal()?;
            state.send(Notice::TerminalSkipped {
                fingerprint,
                ordinal,
                replacement,
            })
        })
        .unwrap_or(Ok(()))
}

#[test]
fn unscoped_event_hooks_preserve_commands_and_shutdown() {
    let registry = GroupRoomRegistry::new(GroupRoomPolicy::new(2, 3, 128, 1_000_000).unwrap());
    let resources = BTreeMap::new();
    let frames = Frames::new();
    let rooms = BTreeMap::new();
    let written = intercept_command(
        Command {
            id: ParticipantId(17),
            event: PeerEvent::Written {
                write_id: 29,
                completed_ns: 43,
            },
        },
        &registry,
        &resources,
        &frames,
        &rooms,
    )
    .unwrap()
    .unwrap();
    assert!(matches!(
        written,
        Command {
            id: ParticipantId(17),
            event: PeerEvent::Written {
                write_id: 29,
                completed_ns: 43
            }
        }
    ));
    let received = intercept_command(
        Command {
            id: ParticipantId(19),
            event: PeerEvent::Received(ServerControlRead {
                message: RoomMessage::Start(StartMessage::Accept(57)),
                captured_ns: 47,
            }),
        },
        &registry,
        &resources,
        &frames,
        &rooms,
    )
    .unwrap()
    .unwrap();
    assert!(matches!(
        received,
        Command {
            id: ParticipantId(19),
            event: PeerEvent::Received(ServerControlRead {
                message: RoomMessage::Start(StartMessage::Accept(57)),
                captured_ns: 47,
            })
        }
    ));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut called = false;
    let error = runtime
        .block_on(shutdown(async {
            called = true;
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "original shutdown result",
            ))
        }))
        .unwrap_err();
    assert!(called);
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(error.to_string(), "original shutdown result");
    // No real Resource is fabricated. Unscoped real-terminal execution and
    // uncalled observers remain the explicit static gaps in PLAN.md.
}

struct ServerGuard {
    stop: Option<oneshot::Sender<()>>,
    notices: Option<mpsc::Receiver<Notice>>,
    completed: completion_channel::Receiver<Result<(), Failure>>,
    completion: Option<Result<(), Failure>>,
    thread: Option<JoinHandle<()>>,
}
impl ServerGuard {
    fn spawn(options: ServerOptions, key: String, identity: Vec<u8>) -> io::Result<Self> {
        let (stop, stopped) = oneshot::channel();
        let (notices, received) = mpsc::channel(16);
        let (finished, completed) = completion_channel::sync_channel(1);
        let thread = thread::Builder::new()
            .name("original-group-serve".to_owned())
            .spawn(move || {
                let outcome = catch_unwind(AssertUnwindSafe(|| -> io::Result<()> {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()?;
                    let result = runtime.block_on(async move {
                        let config = configuration(&options)?;
                        let state = ControlState {
                            key,
                            old_identity: identity,
                            notices,
                            stop: Some(stopped),
                            token: None,
                            barrier_sent: false,
                            replayed: false,
                            applied: false,
                            terminal: None,
                            terminal_passed: false,
                            baseline: None,
                            ordinal: 0,
                        };
                        CONTROL
                            .scope(RefCell::new(state), super::serve(options, config))
                            .await
                    });
                    // A completion notice is never evidence of a still-live runtime.
                    drop(runtime);
                    result
                }));
                let result = match outcome {
                    Ok(result) => result.map_err(|error| Failure::capture(&error)),
                    Err(_) => Err(Failure {
                        kind: io::ErrorKind::Other,
                        text: "owned server thread unwound after runtime disposal".to_owned(),
                    }),
                };
                let _ = finished.try_send(result);
            })?;
        Ok(Self {
            stop: Some(stop),
            notices: Some(received),
            completed,
            completion: None,
            thread: Some(thread),
        })
    }
    fn poll(&mut self) -> io::Result<Option<Notice>> {
        match self
            .notices
            .as_mut()
            .ok_or_else(|| invalid("server notices disposed"))?
            .try_recv()
        {
            Ok(notice) => Ok(Some(notice)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => {
                Err(invalid("actual server notice channel disconnected"))
            }
        }
    }
    fn completion_error(&mut self) -> Option<io::Error> {
        if self.completion.is_none() {
            if let Ok(completion) = self.completed.try_recv() {
                self.completion = Some(completion);
            }
        }
        self.completion.as_ref().map(|result| match result {
            Ok(()) => invalid("actual server completed before fixture shutdown"),
            Err(error) => error.error(),
        })
    }
    fn request_stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        // Disposing pending notices also releases any not-yet-collected terminal
        // gate; no Resource is stored in this receiver, only its release sender.
        self.notices.take();
    }
    fn finish(&mut self) -> Vec<Failure> {
        self.request_stop();
        let mut errors = Vec::new();
        if self.completion.is_none() {
            match self.completed.recv_timeout(WAIT + CLOSE) {
                Ok(completion) => self.completion = Some(completion),
                Err(error) => errors.push(Failure {
                    kind: io::ErrorKind::TimedOut,
                    text: format!("server runtime disposal completion not received: {error}"),
                }),
            }
        }
        // Rust provides no bounded join. The owning test process has the hard
        // deadline; returning without this join would conceal a leaked actor.
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                errors.push(Failure {
                    kind: io::ErrorKind::Other,
                    text: "owned actual server thread join panicked".to_owned(),
                });
            }
        }
        if self.completion.is_none() {
            if let Ok(completion) = self.completed.try_recv() {
                self.completion = Some(completion);
            }
        }
        if let Some(Err(error)) = &self.completion {
            errors.push(error.clone());
        }
        errors
    }
}
impl Drop for ServerGuard {
    fn drop(&mut self) {
        if self.thread.is_some() {
            for error in self.finish() {
                eprintln!("server cleanup: {:?}: {}", error.kind, error.text);
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct History {
    id: ParticipantId,
    identity: Vec<u8>,
    members: Vec<GroupRoomMember>,
    phase: GroupRoomPhase,
    deadline_ns: Option<i64>,
    schedule: Option<StartSchedule>,
}
struct FenceEvidence {
    first: Failure,
    history: History,
    repeated: Option<Failure>,
    schedule: Result<Option<StartSchedule>, Failure>,
    progress: Result<(), Failure>,
}
impl FenceEvidence {
    fn verify(&self, expected: &History) -> io::Result<()> {
        check(
            self.first.kind != io::ErrorKind::TimedOut
                && self.first.text != "room stream driver stopped",
            "survivor failure was timeout or manufactured local stop",
        )?;
        check(
            &self.history == expected,
            "failed survivor lost immutable public history",
        )?;
        check(
            self.repeated.as_ref() == Some(&self.first),
            "first public driver error changed",
        )?;
        check(
            self.schedule.as_ref().err() == Some(&self.first),
            "fenced schedule replaced first public error",
        )?;
        check(
            self.progress.as_ref().err() == Some(&self.first),
            "fenced progress replaced first public error",
        )
    }
}
struct Peer {
    io: Option<RoomPlayIo<WebTransportStream>>,
    retired: Option<WebTransportStream>,
    origin: LocalInstant,
    schedule: Option<StartSchedule>,
    first_failure: Option<Failure>,
    finish_result: Option<Result<(), Failure>>,
    players: [PlayerId; 2],
    preroll: i64,
}
impl Peer {
    fn connect(
        config: &WebTransportOptions,
        identity: &[u8],
        players: [PlayerId; 2],
        preroll: i64,
    ) -> io::Result<Self> {
        let io = WebTransportEndpoint::prepare(config)?.connect_room_play(
            identity,
            &players,
            StartPolicy::default(),
            preroll,
            &AtomicBool::new(false),
            LocalInstant::now() + WAIT,
        )?;
        Ok(Self {
            io: Some(io),
            retired: None,
            origin: LocalInstant::now(),
            schedule: None,
            first_failure: None,
            finish_result: None,
            players,
            preroll,
        })
    }
    fn owner(&self) -> io::Result<&RoomPlayIo<WebTransportStream>> {
        self.io
            .as_ref()
            .ok_or_else(|| invalid("protocol owner already closed"))
    }
    fn owner_mut(&mut self) -> io::Result<&mut RoomPlayIo<WebTransportStream>> {
        self.io
            .as_mut()
            .ok_or_else(|| invalid("protocol owner already closed"))
    }
    fn service(&mut self, failures_expected: bool) -> io::Result<()> {
        let Some(owner) = &mut self.io else {
            if let Some(stream) = &self.retired {
                stream.idle(Duration::from_micros(50));
            }
            return Ok(());
        };
        if self.first_failure.is_some() || owner.session().leave_written() {
            owner.stream_mut().idle(Duration::from_micros(50));
            return Ok(());
        }
        let origin = self.origin;
        let stepped =
            owner.step(|| i64::try_from(origin.elapsed().as_nanos()).map_err(public_error));
        if let Err(error) = stepped {
            self.first_failure = Some(Failure::capture(&error));
            if !failures_expected || error.kind() == io::ErrorKind::TimedOut {
                return Err(error);
            }
            return Ok(());
        }
        if !owner.session().leave_written() {
            if let Some(schedule) = owner.take_schedule()? {
                check(
                    self.schedule.replace(schedule).is_none(),
                    "duplicate actual client Commit schedule",
                )?;
                check(
                    schedule.song_target_ns.checked_sub(schedule.target_ns) == Some(self.preroll)
                        && schedule.target_ns > 0
                        && schedule.uncertainty_ns <= StartPolicy::default().max_uncertainty_ns,
                    "actual client schedule lost local clock/preroll provenance",
                )?;
                check(
                    owner.session().start_committed(),
                    "schedule lacks an actual matching client Commit",
                )?;
            }
        }
        Ok(())
    }
    fn history(&self) -> io::Result<History> {
        let session = self.owner()?.session();
        let room = session
            .room()
            .ok_or_else(|| invalid("public client room absent"))?;
        Ok(History {
            id: session
                .participant()
                .ok_or_else(|| invalid("public participant absent"))?,
            identity: room.identity.to_vec(),
            members: room.members.to_vec(),
            phase: room.phase,
            deadline_ns: room.deadline_ns,
            schedule: self.schedule,
        })
    }
    fn close(&mut self) {
        if self.finish_result.is_some() {
            return;
        }
        if let Some(mut owner) = self.io.take() {
            owner.stop();
            self.retired = Some(owner.into_stream());
        }
        self.finish_result = Some(match &mut self.retired {
            Some(stream) => stream
                .finish(CLOSE)
                .map_err(|error| Failure::capture(&error)),
            None => Ok(()),
        });
    }
    fn fence_evidence(&mut self) -> io::Result<FenceEvidence> {
        let first = self
            .first_failure
            .clone()
            .ok_or_else(|| invalid("survivor has no independent genuine failure"))?;
        let history = self.history()?;
        let repeated = self
            .owner_mut()?
            .step(|| Ok(0))
            .err()
            .map(|error| Failure::capture(&error));
        let schedule = self
            .owner_mut()?
            .take_schedule()
            .map_err(|error| Failure::capture(&error));
        let members = prefix(self.players, 1);
        let progress = self
            .owner_mut()?
            .publish_progress(&members, false)
            .map_err(|error| Failure::capture(&error));
        Ok(FenceEvidence {
            first,
            history,
            repeated,
            schedule,
            progress,
        })
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.close();
    }
}

struct FixtureGuard {
    old: Vec<Peer>,
    replacement: Vec<Peer>,
    pending: Vec<Notice>,
    original: Option<Command>,
    gate: Option<oneshot::Sender<()>>,
    commands: Option<mpsc::Sender<Command>>,
    server: ServerGuard,
    allow_old_failures: bool,
    finished: bool,
}
impl FixtureGuard {
    fn new(options: ServerOptions, key: String, identity: Vec<u8>) -> io::Result<Self> {
        Ok(Self {
            old: Vec::new(),
            replacement: Vec::new(),
            pending: Vec::new(),
            original: None,
            gate: None,
            commands: None,
            server: ServerGuard::spawn(options, key, identity)?,
            allow_old_failures: false,
            finished: false,
        })
    }
    fn service(&mut self) -> io::Result<()> {
        for peer in &mut self.old {
            peer.service(self.allow_old_failures)?;
        }
        for peer in &mut self.replacement {
            peer.service(false)?;
        }
        if let Some(error) = self.server.completion_error() {
            return Err(error);
        }
        Ok(())
    }
    fn collect_notices(&mut self) -> io::Result<()> {
        while let Some(notice) = self.server.poll()? {
            self.pending.push(notice);
        }
        Ok(())
    }
    fn until(&mut self, label: &str, mut done: impl FnMut(&Self) -> bool) -> io::Result<()> {
        let deadline = LocalInstant::now() + WAIT;
        loop {
            self.collect_notices()?;
            if done(self) {
                return Ok(());
            }
            check(
                LocalInstant::now() < deadline,
                &format!("deadline waiting for positive {label}"),
            )?;
            self.service()?;
        }
    }
    fn notice(&mut self, label: &str, matches: impl Fn(&Notice) -> bool) -> io::Result<Notice> {
        self.until(label, |fixture| fixture.pending.iter().any(&matches))?;
        let index = self
            .pending
            .iter()
            .position(matches)
            .ok_or_else(|| invalid("positive notice disappeared"))?;
        Ok(self.pending.remove(index))
    }
    fn replay(&mut self) -> io::Result<()> {
        let command = self
            .original
            .take()
            .ok_or_else(|| invalid("original Written already requeued or absent"))?;
        // No fabricated Command, new timestamp or retry: the original value is
        // moved exactly once into the existing production command channel.
        self.commands
            .as_ref()
            .ok_or_else(|| invalid("actor command sender absent"))?
            .try_send(command)
            .map_err(|error| invalid(format!("fixture original replay: {error}")))
    }
    fn release_terminal(&mut self) -> io::Result<()> {
        self.gate
            .take()
            .ok_or_else(|| invalid("original terminal gate absent"))?
            .send(())
            .map_err(|_| invalid("original terminal gate receiver disappeared"))
    }
    fn close_old(&mut self) {
        for peer in &mut self.old {
            peer.close();
        }
    }
    fn finish(&mut self) -> Vec<Failure> {
        if self.finished {
            return Vec::new();
        }
        self.finished = true;
        if let Some(gate) = self.gate.take() {
            let _ = gate.send(());
        }
        self.original.take();
        self.pending.clear();
        self.server.request_stop();
        for peer in self.old.iter_mut().chain(&mut self.replacement) {
            peer.close();
        }
        let mut errors: Vec<_> = self
            .old
            .iter()
            .chain(&self.replacement)
            .filter_map(|peer| match &peer.finish_result {
                Some(Err(error)) if error.kind == io::ErrorKind::TimedOut => Some(error.clone()),
                None => Some(Failure {
                    kind: io::ErrorKind::Other,
                    text: "owned client close was not attempted".to_owned(),
                }),
                _ => None,
            })
            .collect();
        for peer in self.old.iter_mut().chain(&mut self.replacement) {
            peer.retired.take();
        }
        self.commands.take();
        errors.extend(self.server.finish());
        errors
    }
}
impl Drop for FixtureGuard {
    fn drop(&mut self) {
        for error in self.finish() {
            eprintln!("fixture cleanup: {:?}: {}", error.kind, error.text);
        }
    }
}

fn selected_identity(seed: u64, players: [PlayerId; 2]) -> io::Result<Vec<u8>> {
    let source = beatkernel_bms::parse_seeded(
        "#BPM 60\n#TOTAL 320\n#WAV01 note.wav\n#00111:0101\n",
        Default::default(),
        seed,
    )
    .map_err(public_error)?;
    let policy = ResolvedPlayPolicy::bms(
        &source,
        BmsGaugeKind::Hard,
        &[ClassifiedWindow {
            judgment: BmsJudgment::Great,
            window: JudgeWindow {
                grade: JudgeGrade(7),
                early: SongDuration::from_nanos(7),
                late: SongDuration::from_nanos(9),
            },
        }],
        -3,
    )
    .map_err(public_error)?;
    let prepared = PreparedBms {
        compiled: source.compile().map_err(public_error)?,
        source,
        bank: SampleBank::new(
            AudioFormat::new(1000, 1).map_err(public_error)?,
            PcmLimits::new(64, 256, 4).map_err(public_error)?,
        )
        .map_err(public_error)?,
        sounds: Vec::new(),
        bgm_commands: Vec::new(),
    };
    let assignments = [(players[0], DeviceId(31)), (players[1], DeviceId(32))];
    let bindings = BTreeMap::from([(0x11, 7)]);
    let start = Timestamp::from_nanos(1);
    let end = Some(Timestamp::from_nanos(5_000_000_000));
    let cohort = prepare_audio_cohort_with_policy(
        &prepared,
        &assignments,
        &CompetitionOptions::default(),
        &CohortPreparation {
            host: ClockDomainId(99),
            output: ClockDomainId(2),
            early: 7,
            late: 9,
            offset: -3,
            preroll: 0,
            start,
            end,
            chart_seed: seed,
            bindings: &bindings,
            record_replay: None,
            replay_max_bytes: 0,
            replay_max_records: 0,
        },
        ClockDomainId(17),
        &policy,
    )
    .map_err(public_error)?;
    check(
        cohort.configs.len() == 2 && cohort.states.len() == 2,
        "selected preparation did not create both players",
    )?;
    let policies = [(players[0], &policy), (players[1], &policy)];
    check(
        NativeGroupCompetition::prepare_with_policies(
            &CompetitionOptions::default(),
            &prepared.source,
            &cohort.configs,
            &policies,
            ClockDomainId(17),
            start,
            seed,
            end,
            0,
        )
        .map_err(public_error)?
        .is_none(),
        "genuine public selected admission unexpectedly acquired a network",
    )?;
    let mut identity = None;
    for (member, state) in cohort.configs.iter().zip(&cohort.states) {
        check(
            players.contains(&member.player)
                && member.judge.effective_song_time().is_none()
                && state.capture.is_none()
                && state.gauge.profile() == policy.gauge(),
            "selected member preparation provenance changed",
        )?;
        let actual = prepare_policy_competition_identity(
            &prepared.source,
            &member.judge,
            &policy,
            ClockDomainId(17),
            start,
            seed,
            end,
        )
        .map_err(public_error)?;
        if let Some(expected) = &identity {
            check(
                expected == &actual,
                "selected members have different identities",
            )?;
        } else {
            identity = Some(actual);
        }
    }
    identity.ok_or_else(|| invalid("selected cohort has no canonical identity"))
}

fn prefix(players: [PlayerId; 2], sequence: u64) -> Vec<MemberProgress> {
    players
        .into_iter()
        .map(|player| MemberProgress {
            player,
            progress: Progress {
                song_ns: sequence as i64 * 1_000_000_000,
                hits: sequence * 4,
                misses: sequence,
                combo: sequence,
                max_combo: sequence * 2,
            },
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Case {
    Normal,
    Leave,
    Disconnect,
}

fn environment_path(name: &str) -> io::Result<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).ok_or_else(|| {
        invalid(format!(
            "ignored actor case requires fresh test-child {name}"
        ))
    })
}
fn fixture_options(case: Case) -> io::Result<(ServerOptions, WebTransportOptions, String)> {
    // ServerOptions rejects port zero. The actual Endpoint bind remains the
    // authority; a competing bind after this probe is a real startup failure.
    let probe = UdpSocket::bind("127.0.0.1:0")?;
    let bind = probe.local_addr()?;
    drop(probe);
    let origin = std::env::var("BEATKERNEL_TEST_WEBTRANSPORT_ORIGIN").map_err(public_error)?;
    let name = match case {
        Case::Normal => "original",
        Case::Leave => "leave",
        Case::Disconnect => "disconnect",
    };
    let key = format!("server_commit_{}_{name}", std::process::id());
    let server = ServerOptions {
        bind,
        cert: environment_path("BEATKERNEL_TEST_QUIC_CERT")?,
        key: environment_path("BEATKERNEL_TEST_QUIC_KEY")?,
        origins: vec![origin.clone()],
        allow_missing_origin: false,
        max_rooms: 2,
        max_key_bytes: 128,
        max_sessions: 8,
        max_setups: 4,
        group_hosts: Some(3),
        waiting_ttl: Duration::from_secs(30),
        setup_timeout: WAIT,
        io_timeout: WAIT,
    };
    let client = WebTransportOptions {
        url: format!("https://127.0.0.1:{}/rooms/{key}", bind.port()),
        origin,
        ca: environment_path("BEATKERNEL_TEST_QUIC_CA")?,
        role: StartRole::Join,
    };
    Ok((server, client, key))
}
fn peers(fixture: &FixtureGuard, replacement: bool) -> &[Peer] {
    if replacement {
        &fixture.replacement
    } else {
        &fixture.old
    }
}
fn peers_mut(fixture: &mut FixtureGuard, replacement: bool) -> &mut [Peer] {
    if replacement {
        &mut fixture.replacement
    } else {
        &mut fixture.old
    }
}
fn room_matches(
    peer: &Peer,
    identity: &[u8],
    roster: &[GroupRoomMember],
    phase: GroupRoomPhase,
    deadline: Option<i64>,
) -> bool {
    peer.io.as_ref().is_some_and(|owner| {
        owner.session().room().is_some_and(|room| {
            room.identity == identity
                && room.members == roster
                && room.phase == phase
                && room.deadline_ns == deadline
        })
    })
}
fn collecting(
    fixture: &mut FixtureGuard,
    client: &WebTransportOptions,
    seed: u64,
    replacement: bool,
) -> io::Result<(Vec<u8>, Vec<GroupRoomMember>, i64)> {
    let mut roster = Vec::<GroupRoomMember>::new();
    let mut identity = None;
    let mut room_deadline = None;
    for index in 0..3 {
        let players = [
            PlayerId(11 + 10 * index as u32),
            PlayerId(12 + 10 * index as u32),
        ];
        let actual_identity = selected_identity(seed, players)?;
        if let Some(expected) = &identity {
            check(
                expected == &actual_identity,
                "three genuinely selected hosts disagree",
            )?;
        } else {
            identity = Some(actual_identity.clone());
        }
        let peer = Peer::connect(client, &actual_identity, players, PREROLL[index])?;
        if replacement {
            fixture.replacement.push(peer);
        } else {
            fixture.old.push(peer);
        }
        fixture.until("actual ordered new participant admission", |fixture| {
            peers(fixture, replacement).get(index).is_some_and(|peer| {
                peer.io.as_ref().is_some_and(|owner| {
                    owner.session().participant().is_some() && owner.session().room().is_some()
                })
            })
        })?;
        let session = peers(fixture, replacement)[index].owner()?.session();
        let id = session
            .participant()
            .ok_or_else(|| invalid("actual new ID absent"))?;
        check(
            id.0 != 0 && roster.iter().all(|member| member.id != id),
            "actual admission reused or zero ID",
        )?;
        let deadline = session
            .room()
            .and_then(|room| room.deadline_ns)
            .ok_or_else(|| invalid("Collecting deadline absent"))?;
        if let Some(expected) = room_deadline {
            check(expected == deadline, "Collecting waiting deadline changed")?;
        } else {
            room_deadline = Some(deadline);
        }
        roster.push(GroupRoomMember {
            id,
            players: players.to_vec(),
            prepared: false,
        });
        fixture.until("every exact public Collecting roster", |fixture| {
            peers(fixture, replacement).iter().all(|peer| {
                room_matches(
                    peer,
                    &actual_identity,
                    &roster,
                    GroupRoomPhase::Collecting,
                    Some(deadline),
                ) && peer.schedule.is_none()
            })
        })?;
    }
    Ok((
        identity.ok_or_else(|| invalid("host identity absent"))?,
        roster,
        room_deadline.ok_or_else(|| invalid("room deadline absent"))?,
    ))
}
fn ready_except_last(
    fixture: &mut FixtureGuard,
    replacement: bool,
    identity: &[u8],
    roster: &mut [GroupRoomMember],
    deadline: i64,
) -> io::Result<()> {
    peers_mut(fixture, replacement)[0]
        .owner_mut()?
        .request_seal()?;
    fixture.until("actual Seal and complete Frozen roster", |fixture| {
        peers(fixture, replacement).iter().all(|peer| {
            room_matches(
                peer,
                identity,
                roster,
                GroupRoomPhase::Frozen,
                Some(deadline),
            )
        })
    })?;
    for index in 0..2 {
        peers_mut(fixture, replacement)[index]
            .owner_mut()?
            .request_ready()?;
        roster[index].prepared = true;
        fixture.until(
            "positive exact partial Ready before final Ready",
            |fixture| {
                peers(fixture, replacement).iter().all(|peer| {
                    room_matches(
                        peer,
                        identity,
                        roster,
                        GroupRoomPhase::Frozen,
                        Some(deadline),
                    ) && peer.schedule.is_none()
                })
            },
        )?;
    }
    Ok(())
}
fn final_ready(
    fixture: &mut FixtureGuard,
    replacement: bool,
    roster: &mut [GroupRoomMember],
) -> io::Result<()> {
    peers_mut(fixture, replacement)[2]
        .owner_mut()?
        .request_ready()?;
    roster[2].prepared = true;
    Ok(())
}
fn committed_clients(
    fixture: &FixtureGuard,
    replacement: bool,
    identity: &[u8],
    roster: &[GroupRoomMember],
) -> bool {
    let clients = peers(fixture, replacement);
    clients.len() == 3
        && clients.iter().all(|peer| {
            peer.schedule.is_some()
                && room_matches(peer, identity, roster, GroupRoomPhase::Prepared, None)
                && peer
                    .io
                    .as_ref()
                    .is_some_and(|owner| owner.session().start_committed())
        })
}
fn complete_exchange(fixture: &mut FixtureGuard, replacement: bool) -> io::Result<()> {
    let ids: Vec<_> = peers(fixture, replacement)
        .iter()
        .map(|peer| peer.history().map(|history| history.id))
        .collect::<io::Result<_>>()?;
    for sequence in 1..=3 {
        let final_prefix = sequence == 3;
        let prefixes: Vec<_> = peers(fixture, replacement)
            .iter()
            .map(|peer| prefix(peer.players, sequence))
            .collect();
        for (peer, prefix) in peers_mut(fixture, replacement).iter_mut().zip(&prefixes) {
            peer.owner_mut()?.publish_progress(prefix, final_prefix)?;
        }
        fixture.until("every directed original native progress/final", |fixture| {
            peers(fixture, replacement)
                .iter()
                .enumerate()
                .all(|(receiver, peer)| {
                    ids.iter().enumerate().all(|(sender, id)| {
                        receiver == sender
                            || peer.io.as_ref().is_some_and(|owner| {
                                owner.peer_progress(*id).is_some_and(|actual| {
                                    actual.sequence == sequence
                                        && actual.final_prefix == final_prefix
                                        && actual.members == prefixes[sender]
                                })
                            })
                    })
                })
        })?;
    }
    fixture.until("all actual final application ACKs", |fixture| {
        peers(fixture, replacement).iter().all(|peer| {
            peer.io.as_ref().is_some_and(|owner| {
                owner.progress_complete()
                    && owner.local_final_written()
                    && owner.local_final_acknowledged()
            })
        })
    })?;
    for (receiver, peer) in peers(fixture, replacement).iter().enumerate() {
        let owner = peer.owner()?;
        check(
            !owner.drain_complete(),
            "final ACKs fabricated drain completion",
        )?;
        check(
            ids.iter()
                .enumerate()
                .all(|(sender, id)| sender == receiver || owner.peer_final_ack_written(*id)),
            "directed final receipt lacks genuine ACK write",
        )?;
    }
    for peer in peers_mut(fixture, replacement) {
        peer.owner_mut()?.request_drain()?;
    }
    fixture.until("explicit actual coordinated DrainComplete", |fixture| {
        peers(fixture, replacement)
            .iter()
            .all(|peer| peer.io.as_ref().is_some_and(|owner| owner.drain_complete()))
    })?;
    // Collect every finish result before inspecting any one of them. Native
    // finish is a public drain.and(closed) result, not proof of each inner drain.
    for peer in peers_mut(fixture, replacement) {
        peer.close();
    }
    Ok(())
}
fn check_closes(clients: &[Peer], cancelled: bool) -> io::Result<()> {
    for peer in clients {
        match peer
            .finish_result
            .as_ref()
            .ok_or_else(|| invalid("owned client close not attempted"))?
        {
            Ok(()) => {}
            Err(failure) => {
                check(
                    failure.kind != io::ErrorKind::TimedOut,
                    "actual native close returned timeout",
                )?;
                if !cancelled {
                    return Err(failure.error());
                }
                check(
                    matches!(
                        failure.kind,
                        io::ErrorKind::ConnectionAborted
                            | io::ErrorKind::ConnectionReset
                            | io::ErrorKind::BrokenPipe
                            | io::ErrorKind::NotConnected
                            | io::ErrorKind::UnexpectedEof
                    ),
                    "cancelled client close returned unexpected public failure",
                )?;
            }
        }
    }
    Ok(())
}
fn replacement_snapshot(identity: &[u8], roster: &[GroupRoomMember], deadline: i64) -> Snapshot {
    let mut ids: Vec<_> = roster.iter().map(|member| member.id).collect();
    ids.sort();
    Snapshot {
        identity: identity.to_vec(),
        members: roster.to_vec(),
        phase: GroupRoomPhase::Collecting,
        deadline_ns: Some(deadline),
        resources: ids.clone(),
        frames: ids,
        prepared: false,
    }
}

fn exercise(
    fixture: &mut FixtureGuard,
    client: &WebTransportOptions,
    case: Case,
    bind: SocketAddr,
) -> io::Result<()> {
    match fixture.notice("actual Endpoint started", |notice| {
        matches!(notice, Notice::Started { .. })
    })? {
        Notice::Started {
            bind: actual,
            commands,
        } => {
            check(
                actual == bind && actual.port() != 0,
                "actual Endpoint started on wrong address",
            )?;
            fixture.commands = Some(commands);
        }
        _ => return Err(invalid("wrong Started notice")),
    }
    let (identity, mut roster, deadline) = collecting(fixture, client, 71, false)?;
    ready_except_last(fixture, false, &identity, &mut roster, deadline)?;
    final_ready(fixture, false, &mut roster)?;
    let held = fixture.notice("original actual full Commit Written hold", |notice| {
        matches!(notice, Notice::WrittenHeld { .. })
    })?;
    let token = match held {
        Notice::WrittenHeld { original, state } => {
            let token = state.token;
            check(
                held_commit(&state)
                    && token.id == roster[1].id
                    && token.admitted_ns >= 0
                    && token.completed_ns >= token.admitted_ns,
                "held original Commit token or server state incorrect",
            )?;
            check(
                matches!(&original.event, PeerEvent::Written { write_id, completed_ns }
                if *write_id == token.write_id && *completed_ns == token.completed_ns)
                    && original.id == token.id,
                "notice did not move exact original Written",
            )?;
            fixture.original = Some(original);
            token
        }
        _ => return Err(invalid("wrong WrittenHeld notice")),
    };
    // Wait for BOTH facts in the same serviced loop. A public schedule is
    // emitted only by the common owner's actual matching Commit validation;
    // it never substitutes for the independent server receipt barrier.
    fixture.until(
        "non-target server receipts AND every actual matching client Commit",
        |fixture| {
            committed_clients(fixture, false, &identity, &roster)
                && fixture
                    .pending
                    .iter()
                    .any(|notice| matches!(notice, Notice::OtherCommitReceiptsApplied(_)))
        },
    )?;
    let barrier = fixture
        .pending
        .iter()
        .position(|notice| matches!(notice, Notice::OtherCommitReceiptsApplied(_)))
        .ok_or_else(|| invalid("positive other-receipt barrier absent"))?;
    match fixture.pending.remove(barrier) {
        Notice::OtherCommitReceiptsApplied(state) => check(
            state.token == token
                && held_commit(&state)
                && state.others.len() == 2
                && state.others.iter().all(|(id, admitted, cleared)| {
                    *id != token.id && admitted.is_some() && *cleared
                }),
            "other server receipt barrier was not positive",
        )?,
        _ => return Err(invalid("wrong receipt barrier")),
    }
    if case == Case::Normal {
        // No additional client IO/sleep/factory work between the two positive
        // prerequisites and the real-time replay of the original value.
        fixture.replay()?;
        match fixture.notice("original Written applied by actual actor", |notice| {
            matches!(notice, Notice::WrittenApplied { .. })
        })? {
            Notice::WrittenApplied {
                token: actual,
                ordinal,
                state,
            } => check(
                actual == token
                    && ordinal > 0
                    && state.token == token
                    && state.in_flight.is_none()
                    && state.committed
                    && state.active,
                "actual original receipt did not activate exact room",
            )?,
            _ => return Err(invalid("wrong WrittenApplied notice")),
        }
        complete_exchange(fixture, false)?;
        return Ok(());
    }

    let history: Vec<_> = fixture
        .old
        .iter()
        .map(Peer::history)
        .collect::<io::Result<_>>()?;
    fixture.allow_old_failures = true;
    match case {
        Case::Leave => {
            fixture.old[0].owner_mut()?.request_leave()?;
            fixture.until("another old host's actual Leave write", |fixture| {
                fixture.old[0]
                    .io
                    .as_ref()
                    .is_some_and(|owner| owner.session().leave_written())
            })?;
        }
        Case::Disconnect => fixture.old[0].close(),
        Case::Normal => return Err(invalid("unexpected normal cancellation")),
    }
    fixture.until(
        "each survivor's first genuine failure before local cleanup",
        |fixture| {
            fixture.old[1..]
                .iter()
                .all(|peer| peer.first_failure.is_some())
        },
    )?;
    let fingerprint = match fixture.notice(
        "original actual terminal retained in same JoinSet",
        |notice| matches!(notice, Notice::TerminalHeld { .. }),
    )? {
        Notice::TerminalHeld {
            fingerprint,
            release,
        } => {
            check(
                fingerprint.id == token.id
                    && fingerprint.permits == 1
                    && !fingerprint.stream_present,
                "original terminal Resource/permit fingerprint incorrect",
            )?;
            fixture.gate = Some(release);
            fingerprint
        }
        _ => return Err(invalid("wrong TerminalHeld notice")),
    };
    let (replacement_identity, mut replacement_roster, replacement_deadline) =
        collecting(fixture, client, 72, true)?;
    check(
        replacement_identity != identity
            && replacement_roster
                .iter()
                .all(|member| member.id.0 != 0 && roster.iter().all(|old| old.id != member.id)),
        "same URL replacement did not obtain genuinely different identity/disjoint actual leases",
    )?;
    let expected = replacement_snapshot(
        &replacement_identity,
        &replacement_roster,
        replacement_deadline,
    );
    match fixture.notice(
        "independent after-admission replacement baseline BEFORE either stale release",
        |notice| matches!(notice, Notice::ReplacementAdmitted(_)),
    )? {
        Notice::ReplacementAdmitted(actual) => check(
            actual == expected,
            "independent Collecting baseline differs from public roster/resources/frames",
        )?,
        _ => return Err(invalid("wrong replacement admission notice")),
    }

    let mut last_ordinal = 0;
    for written_first in [case == Case::Leave, case != Case::Leave] {
        if written_first {
            fixture.replay()?;
            match fixture.notice("actual missing-resource Written guard", |notice| {
                matches!(notice, Notice::WrittenSkipped { .. })
            })? {
                Notice::WrittenSkipped {
                    token: actual,
                    ordinal,
                    replacement,
                } => {
                    check(
                        actual == token && ordinal > last_ordinal && replacement == expected,
                        "stale Written token/order/replacement baseline changed",
                    )?;
                    last_ordinal = ordinal;
                }
                _ => return Err(invalid("wrong WrittenSkipped notice")),
            }
        } else {
            fixture.release_terminal()?;
            match fixture.notice(
                "actual stale terminal AFTER original Resource drop",
                |notice| matches!(notice, Notice::TerminalSkipped { .. }),
            )? {
                Notice::TerminalSkipped {
                    fingerprint: actual,
                    ordinal,
                    replacement,
                } => {
                    check(
                        actual == fingerprint && ordinal > last_ordinal && replacement == expected,
                        "stale terminal fingerprint/order/replacement baseline changed",
                    )?;
                    last_ordinal = ordinal;
                }
                _ => return Err(invalid("wrong TerminalSkipped notice")),
            }
        }
    }
    // No replacement Ready is sent before BOTH positive actual dispositions.
    // Every old finish is attempted before any of its result assertions below.
    let fenced: Vec<_> = fixture.old[1..]
        .iter_mut()
        .map(Peer::fence_evidence)
        .collect::<io::Result<_>>()?;
    fixture.close_old();
    for (evidence, expected) in fenced.iter().zip(&history[1..]) {
        evidence.verify(expected)?;
    }
    check_closes(&fixture.old, true)?;
    ready_except_last(
        fixture,
        true,
        &replacement_identity,
        &mut replacement_roster,
        replacement_deadline,
    )?;
    final_ready(fixture, true, &mut replacement_roster)?;
    fixture.until(
        "replacement's later genuine measured Ready/start",
        |fixture| committed_clients(fixture, true, &replacement_identity, &replacement_roster),
    )?;
    complete_exchange(fixture, true)?;
    Ok(())
}
fn run_case(case: Case) {
    let (server, client, key) =
        fixture_options(case).expect("fresh strict actor fixture configuration");
    let bind = server.bind;
    let identity = selected_identity(71, [PlayerId(11), PlayerId(12)])
        .expect("genuine selected original identity");
    let mut fixture =
        FixtureGuard::new(server, key, identity).expect("owned actual server thread creation");
    let primary = exercise(&mut fixture, &client, case, bind);
    // Store all public results while owners still exist, then dispose every
    // client/runtime and join the server before final result assertions.
    let old_closes = if primary.is_ok() {
        check_closes(&fixture.old, case != Case::Normal)
    } else {
        Ok(())
    };
    let replacement_closes = if primary.is_ok() {
        check_closes(&fixture.replacement, false)
    } else {
        Ok(())
    };
    let cleanup = fixture.finish();
    if let Err(error) = primary {
        for peer in fixture.old.iter().chain(&fixture.replacement) {
            if let Some(Err(failure)) = &peer.finish_result {
                eprintln!(
                    "secondary public client finish: {:?}: {}",
                    failure.kind, failure.text
                );
            }
        }
        for failure in cleanup {
            eprintln!("secondary cleanup: {:?}: {}", failure.kind, failure.text);
        }
        panic!(
            "original real actor fixture failure: {:?}: {}",
            error.kind(),
            error
        );
    }
    old_closes.expect("all original public finish results checked after every attempt");
    replacement_closes.expect("all replacement public finish results checked after every attempt");
    assert!(
        cleanup.is_empty(),
        "owned cleanup/runtime-disposal/join errors: {cleanup:?}"
    );
}

#[test]
#[ignore = "requires fresh strict PKI in test-child environment and an owned process deadline"]
fn real_server_original_commit_receipt_activates_exact_room_after_actual_commit_write() {
    run_case(Case::Normal);
}
#[test]
#[ignore = "requires fresh strict PKI in test-child environment and an owned process deadline"]
fn real_server_leave_replacement_survives_written_then_terminal() {
    run_case(Case::Leave);
}
#[test]
#[ignore = "requires fresh strict PKI in test-child environment and an owned process deadline"]
fn real_server_disconnect_replacement_survives_terminal_then_written() {
    run_case(Case::Disconnect);
}
