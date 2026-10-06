//! Portable room comparison and lifecycle policy over explicit network/UI/runtime ports.

use crate::{
    local_players::PlayerId,
    multiplayer_group::{validate_members, validate_roster, MemberProgress},
    multiplayer_group_rooms::GroupRoomPhase,
    multiplayer_start::StartSchedule,
    room_network_model::{
        RoomNetworkPort, RoomCommand, RoomFailure, RoomOutcome, RoomPoll, RoomReply, RoomRoster,
        RoomSnapshot,
    },
    room_opponent_hud::{RoomHudStatus, RoomOpponentHud},
    room_ui_host::RoomUiHost,
    room_runtime_host::RoomRuntimeHost,
    room_presentation::{
        RoomLobby, RoomPresentation, RoomResults, RoomStatus, RoomUiAction, RoomUiReply,
        ROOM_UI_CAPACITY,
    },
};
use std::{collections::VecDeque, io, sync::Arc, time::Duration};

const LOBBY_LIMIT: usize = 16;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
fn from_failure(error: &RoomFailure) -> io::Error {
    io::Error::new(error.kind, error.message.clone())
}
fn copy_members(members: &[MemberProgress]) -> io::Result<Vec<MemberProgress>> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(members.len())
        .map_err(io::Error::other)?;
    result.extend_from_slice(members);
    Ok(result)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommandKind {
    Seal,
    Ready,
    Leave,
    Progress,
    Final,
    Drain,
}
impl CommandKind {
    fn lobby(self) -> bool {
        matches!(self, Self::Seal | Self::Ready | Self::Leave)
    }
}
struct Pending {
    id: u64,
    kind: CommandKind,
}
struct UiPending {
    id: u64,
    network: Option<u64>,
    result: Option<Result<(), String>>,
}
fn ui_message(message: &str) -> String {
    message
        .chars()
        .take(512)
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

/// Owns one actual local roster, one portable retained HUD and bounded command
/// correlations. A room snapshot alone never authorizes native audio startup.
pub struct RoomCompetition<P: RoomNetworkPort, H: RoomUiHost, R: RoomRuntimeHost> {
    runtime: R,
    host: H,
    port: P,
    players: Vec<PlayerId>,
    snapshot: RoomSnapshot,
    prepared: Option<Arc<RoomRoster>>,
    hud: Option<RoomOpponentHud>,
    hud_error: Option<String>,
    peer_sequences: Vec<(crate::multiplayer_rooms::ParticipantId, u64)>,
    local: Option<Vec<MemberProgress>>,
    pending: Vec<Pending>,
    replies: VecDeque<RoomReply>,
    last_id: Option<u64>,
    last_clock: Option<i64>,
    publication: crate::competition_progress_cadence::ProgressCadence,
    failure: Option<RoomFailure>,
    finish_timeout: Duration,
    finishing: bool,
    final_accepted: bool,
    drain_accepted: bool,
    leaving: bool,
    cancelled: bool,
    outcome: Option<RoomOutcome>,
    ui_pending: Vec<UiPending>,
    ui_presentation: Option<Arc<RoomPresentation>>,
    ui_dirty: bool,
    ui_error: Option<String>,
}

impl<P: RoomNetworkPort, H: RoomUiHost, R: RoomRuntimeHost> RoomCompetition<P, H, R> {
    pub fn new_with_ports(
        mut port: P,
        players: Vec<PlayerId>,
        finish_timeout: Duration,
        host: H,
        runtime: R,
    ) -> io::Result<Self> {
        let validated = (|| {
            validate_roster(&players).map_err(|error| invalid(error.to_string()))?;
            if finish_timeout < Duration::from_millis(1)
                || finish_timeout > Duration::from_secs(120)
            {
                return Err(invalid("native room finish timeout must be 1 ms..120 s"));
            }
            let mut pending = Vec::new();
            pending
                .try_reserve_exact(LOBBY_LIMIT + 2)
                .map_err(io::Error::other)?;
            let mut replies = VecDeque::new();
            replies
                .try_reserve_exact(LOBBY_LIMIT)
                .map_err(io::Error::other)?;
            let mut peer_sequences = Vec::new();
            peer_sequences
                .try_reserve_exact(63)
                .map_err(io::Error::other)?;
            let mut ui_pending = Vec::new();
            ui_pending
                .try_reserve_exact(ROOM_UI_CAPACITY)
                .map_err(io::Error::other)?;
            Ok((pending, replies, peer_sequences, ui_pending))
        })();
        let (pending, replies, peer_sequences, ui_pending) = match validated {
            Ok(value) => value,
            Err(error) => {
                let outcome = port.stop();
                return Err(match outcome.cleanup_error {
                    Some(cleanup) => {
                        io::Error::new(error.kind(), format!("{error}; cleanup: {cleanup}"))
                    }
                    None => error,
                });
            }
        };
        Ok(Self {
            runtime,
            host,
            port,
            players,
            snapshot: RoomSnapshot::default(),
            prepared: None,
            hud: None,
            hud_error: None,
            peer_sequences,
            local: None,
            pending,
            replies,
            last_id: None,
            last_clock: None,
            publication: crate::competition_progress_cadence::ProgressCadence::new(),
            failure: None,
            finish_timeout,
            finishing: false,
            final_accepted: false,
            drain_accepted: false,
            leaving: false,
            cancelled: false,
            outcome: None,
            ui_pending,
            ui_presentation: None,
            ui_dirty: true,
            ui_error: None,
        })
    }

    pub fn players(&self) -> &[PlayerId] {
        &self.players
    }
    pub fn snapshot(&self) -> &RoomSnapshot {
        &self.snapshot
    }
    pub fn local_progress(&self) -> Option<&[MemberProgress]> {
        self.local.as_deref()
    }
    pub fn hud(&self) -> Option<&RoomOpponentHud> {
        self.hud.as_ref()
    }
    pub fn ui_error(&self) -> Option<&str> {
        self.ui_error.as_deref()
    }
    pub fn hud_error(&self) -> Option<&str> {
        self.hud_error.as_deref()
    }
    pub fn network_error(&self) -> Option<&RoomFailure> {
        self.failure.as_ref()
    }
    pub fn outcome(&self) -> Option<&RoomOutcome> {
        self.outcome.as_ref()
    }
    pub fn take_reply(&mut self) -> Option<RoomReply> {
        self.replies.pop_front()
    }

    pub fn set_page(&mut self, page: usize) -> Result<(), String> {
        let hud = self.hud.as_mut().ok_or("room HUD is not prepared")?;
        let previous = hud.page_index();
        hud.set_page(page)?;
        self.ui_dirty |= previous != page;
        Ok(())
    }
    pub fn disable_hud(&mut self) {
        self.fail_hud("room score presentation was disabled".into());
    }
    fn fail_hud(&mut self, error: String) {
        self.ui_dirty |= self.hud_error.is_none();
        self.hud_error.get_or_insert(error);
        if let Some(hud) = &mut self.hud {
            hud.mark_failed();
        }
    }
    fn hud_status(&mut self, status: RoomHudStatus) {
        if self.hud_error.is_none() {
            if let Some(hud) = &mut self.hud {
                if let Err(error) = hud.set_status(status) {
                    self.fail_hud(error);
                } else if self
                    .ui_presentation
                    .as_ref()
                    .is_none_or(|page| page.heading != hud.heading())
                {
                    self.ui_dirty = true;
                }
            }
        }
    }
    fn fail_network(&mut self, error: io::Error) {
        self.ui_dirty |= self.failure.is_none();
        self.failure.get_or_insert_with(|| error.into());
        self.port.request_stop();
        self.hud_status(RoomHudStatus::Disconnected);
    }
    fn clock(&mut self) -> io::Result<i64> {
        let now = self.port.clock_now_ns()?;
        if now < 0 || self.last_clock.is_some_and(|last| now < last) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "native room elapsed clock regressed",
            ));
        }
        self.last_clock = Some(now);
        Ok(now)
    }
    fn pending(&self, kind: CommandKind) -> bool {
        self.pending.iter().any(|row| row.kind == kind)
    }

    fn send(&mut self, kind: CommandKind, command: RoomCommand) -> io::Result<u64> {
        let id = self.port.try_command(command)?;
        if id == 0 || self.last_id.is_some_and(|previous| id <= previous) {
            let error = io::Error::new(
                io::ErrorKind::InvalidData,
                "native room command identity regressed",
            );
            self.fail_network(io::Error::new(error.kind(), error.to_string()));
            return Err(error);
        }
        self.last_id = Some(id);
        self.pending.push(Pending { id, kind });
        Ok(id)
    }
    fn lobby(&mut self, kind: CommandKind, command: RoomCommand) -> io::Result<u64> {
        if self.finishing
            || self.outcome.is_some()
            || self.leaving
            || self.cancelled
            || self.pending(CommandKind::Leave)
            || self.snapshot.terminal.is_some()
        {
            return Err(invalid("native room lobby is closing"));
        }
        if let Some(error) = &self.failure {
            return Err(from_failure(error));
        }
        if self.replies.len() + self.pending.iter().filter(|row| row.kind.lobby()).count()
            >= LOBBY_LIMIT
        {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "native room lobby replies are full",
            ));
        }
        self.send(kind, command)
    }
    pub fn seal(&mut self) -> io::Result<u64> {
        self.lobby(CommandKind::Seal, RoomCommand::Seal)
    }
    pub fn ready(&mut self) -> io::Result<u64> {
        self.lobby(CommandKind::Ready, RoomCommand::Ready)
    }
    pub fn leave(&mut self) -> io::Result<u64> {
        self.lobby(CommandKind::Leave, RoomCommand::Leave)
    }

    fn accept_poll(&mut self, poll: RoomPoll) -> io::Result<()> {
        self.accept_poll_inner(poll, false)
    }
    fn accept_poll_inner(&mut self, poll: RoomPoll, joined: bool) -> io::Result<()> {
        // Correlated results are validated as a whole before retiring any slot.
        if poll.replies.len() > self.pending.len()
            || poll.replies.iter().enumerate().any(|(index, reply)| {
                !self.pending.iter().any(|row| row.id == reply.id)
                    || poll.replies[..index].iter().any(|old| old.id == reply.id)
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unknown or duplicate native room reply",
            ));
        }
        let mut reply_failure = None;
        for reply in poll.replies {
            let index = self
                .pending
                .iter()
                .position(|row| row.id == reply.id)
                .ok_or_else(|| invalid("native room reply lost its owner"))?;
            let kind = self.pending.remove(index).kind;
            if kind.lobby() {
                if kind == CommandKind::Leave && reply.result.is_ok() {
                    self.leaving = true;
                }
                self.replies.push_back(reply);
            } else {
                match reply.result {
                    Ok(()) => match kind {
                        CommandKind::Final => self.final_accepted = true,
                        CommandKind::Drain => self.drain_accepted = true,
                        _ => {}
                    },
                    Err(error) => {
                        reply_failure.get_or_insert(error);
                    }
                }
            }
        }
        self.ui_dirty |= self.snapshot.revision != poll.snapshot.revision
            || self.snapshot.participant != poll.snapshot.participant
            || self.snapshot.terminal != poll.snapshot.terminal;
        self.snapshot = poll.snapshot;
        if let Some(room) = &self.snapshot.room {
            if room.phase == GroupRoomPhase::Prepared {
                let own = self
                    .snapshot
                    .participant
                    .ok_or_else(|| invalid("prepared room participant is missing"))?;
                let member = room
                    .members
                    .iter()
                    .find(|member| member.id == own)
                    .ok_or_else(|| invalid("prepared room excludes its actual participant"))?;
                if member.players != self.players {
                    return Err(invalid(
                        "prepared room changed the actual ordered local players",
                    ));
                }
                if let Some(prepared) = &self.prepared {
                    if prepared.as_ref() != room.as_ref() {
                        return Err(invalid("prepared room roster changed"));
                    }
                } else {
                    self.prepared = Some(room.clone());
                    if self.hud_error.is_none() {
                        match RoomOpponentHud::new(own, &room.members) {
                            Ok(hud) => self.hud = Some(hud),
                            Err(error) => self.fail_hud(error),
                        }
                    }
                }
            }
        }
        if self.hud_error.is_none() && (joined || self.failure.is_none()) {
            for (participant, prefix) in &self.snapshot.peers {
                let previous = self
                    .peer_sequences
                    .iter()
                    .position(|(id, _)| id == participant);
                if previous.is_some_and(|index| self.peer_sequences[index].1 == prefix.sequence) {
                    continue;
                }
                let Some(hud) = &mut self.hud else {
                    continue;
                };
                let visible = hud.page().iter().any(|row| row.participant == *participant);
                let updated = if joined {
                    hud.retain_after_join(*participant, prefix)
                } else {
                    hud.update(*participant, prefix)
                };
                if let Err(error) = updated {
                    self.hud_error = Some(error);
                    hud.mark_failed();
                    self.ui_dirty = true;
                    break;
                }
                self.ui_dirty |= visible;
                if let Some(index) = previous {
                    self.peer_sequences[index].1 = prefix.sequence;
                } else {
                    self.peer_sequences.push((*participant, prefix.sequence));
                }
            }
        }
        // Apply every newly accepted peer prefix before retiring presentation
        // for a command/terminal failure delivered by the same actual poll.
        if let Some(error) = reply_failure {
            self.fail_network(from_failure(&error));
        }
        if let Some(terminal) = &self.snapshot.terminal {
            let operational = terminal.error.clone();
            let unexpected_cancel = terminal.cancelled && !self.cancelled && !self.leaving;
            self.hud_status(RoomHudStatus::Disconnected);
            if let Some(error) = operational {
                self.fail_network(from_failure(&error));
            } else if unexpected_cancel {
                self.fail_network(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "native room was cancelled",
                ));
            }
        } else if self.snapshot.schedule.is_some() && self.failure.is_none() {
            self.hud_status(RoomHudStatus::Connected);
        }
        Ok(())
    }

    /// Local phase refusals are returned through take_reply, without poisoning
    /// the room. Terminal network failures remain independently inspectable.
    pub fn poll(&mut self) -> io::Result<()> {
        self.service_ui_cancellation();
        if self.cancelled {
            self.publish_ui();
            return Ok(());
        }
        let result = self.poll_network();
        self.publish_ui();
        self.service_ui_requests();
        self.publish_ui();
        result
    }
    fn poll_network(&mut self) -> io::Result<()> {
        if self.outcome.is_some() {
            return Ok(());
        }
        match self.port.poll() {
            Ok(poll) => {
                if let Err(error) = self.accept_poll(poll) {
                    self.fail_network(io::Error::new(error.kind(), error.to_string()));
                    return Err(error);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => {
                self.fail_network(io::Error::new(error.kind(), error.to_string()));
                return Err(error);
            }
        }
        if let Some(error) = &self.failure {
            return Err(from_failure(error));
        }
        // Cleanup has its own terminal field. Do not relabel it as a protocol
        // error merely because the terminal snapshot was observed by polling.
        Ok(())
    }

    fn service_ui_cancellation(&mut self) {
        if self.host.cancelled() && !self.cancelled {
            self.request_stop();
        }
    }

    /// Revoke startup/final authority without joining on a gameplay operation.
    /// Final cleanup still consumes the actual port's joined outcome in finish.
    pub(crate) fn request_stop(&mut self) {
        self.cancelled = true;
        self.port.request_stop();
        self.hud_status(RoomHudStatus::Disconnected);
        self.ui_dirty = true;
        self.host.close_controls();
        self.ui_pending.clear();
        self.publish_ui();
    }
    fn service_ui_requests(&mut self) {
        if !self.host.attached() {
            return;
        }
        self.service_ui_cancellation();
        if self.cancelled || self.finishing || self.outcome.is_some() {
            self.host.close_controls();
            self.ui_pending.clear();
            return;
        }
        // Retain a result under temporary channel contention; never resend its
        // protocol command, and never expose a network identity as a UI identity.
        let mut index = 0;
        while index < self.ui_pending.len() {
            if self.ui_pending[index].result.is_none() {
                if let Some(network) = self.ui_pending[index].network {
                    if let Some(reply) = self.replies.iter().position(|reply| reply.id == network) {
                        let reply = self.replies.remove(reply).expect("located room reply");
                        self.ui_pending[index].result =
                            Some(reply.result.map_err(|error| ui_message(&error.to_string())));
                    }
                }
            }
            if let Some(result) = self.ui_pending[index].result.as_ref() {
                let reply = RoomUiReply {
                    id: self.ui_pending[index].id,
                    result: result.clone(),
                };
                match self.host.reply(reply) {
                    Ok(()) => {
                        self.ui_pending.remove(index);
                        continue;
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) => {
                        self.ui_error
                            .get_or_insert_with(|| ui_message(&error.to_string()));
                        self.host.close_controls();
                        self.ui_pending.clear();
                        self.ui_dirty = true;
                        return;
                    }
                }
            }
            index += 1;
        }
        for _ in self.ui_pending.len()..ROOM_UI_CAPACITY {
            let request = match self.host.take_request() {
                Ok(Some(request)) => request,
                Ok(None) => break,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => {
                    self.ui_error
                        .get_or_insert_with(|| ui_message(&error.to_string()));
                    self.ui_dirty = true;
                    break;
                }
            };
            if self.host.cancelled() {
                self.service_ui_cancellation();
                break;
            }
            let allowed = self
                .ui_presentation
                .as_ref()
                .is_some_and(|page| page.allows(request.action));
            let mut pending = UiPending {
                id: request.id,
                network: None,
                result: None,
            };
            if !allowed {
                pending.result =
                    Some(Err("room action is unavailable in the current owner".into()));
            } else {
                let result = match request.action {
                    RoomUiAction::Seal => self.seal().map(Some),
                    RoomUiAction::Ready => self.ready().map(Some),
                    RoomUiAction::Leave => self.leave().map(Some),
                    RoomUiAction::Page(page) => self.set_page(page).map(|_| None).map_err(invalid),
                };
                match result {
                    Ok(Some(network)) => pending.network = Some(network),
                    Ok(None) => pending.result = Some(Ok(())),
                    Err(error) => pending.result = Some(Err(ui_message(&error.to_string()))),
                }
            }
            // Immediate local page/refusal results have no network receipt to await.
            if let Some(result) = pending.result.as_ref() {
                if self
                    .host
                    .reply(RoomUiReply {
                        id: pending.id,
                        result: result.clone(),
                    })
                    .is_ok()
                {
                    continue;
                }
            }
            self.ui_pending.push(pending);
        }
    }
    fn publish_ui(&mut self) {
        if !self.host.attached() {
            return;
        }
        let status = if self.finishing
            || self.outcome.is_some()
            || self.cancelled
            || self.leaving
            || self.pending(CommandKind::Leave)
            || self.pending(CommandKind::Progress)
        {
            RoomStatus::Closed
        } else if self.failure.is_some() || self.snapshot.terminal.is_some() {
            RoomStatus::Disconnected
        } else if self.snapshot.schedule.is_some() {
            RoomStatus::Connected
        } else {
            RoomStatus::Waiting
        };
        self.ui_dirty |= self
            .ui_presentation
            .as_ref()
            .is_none_or(|old| old.status != status);
        if self.ui_dirty {
            let build = (|| -> Result<Arc<RoomPresentation>, String> {
                let lobby = if let Some(old) = self.ui_presentation.as_ref().filter(|old| {
                    old.lobby.revision == self.snapshot.revision
                        && old.lobby.participant == self.snapshot.participant
                }) {
                    old.lobby.clone()
                } else {
                    let members = self
                        .snapshot
                        .room
                        .as_ref()
                        .map_or_else(Vec::new, |room| room.members.clone());
                    Arc::new(RoomLobby::new(
                        self.snapshot.participant,
                        self.snapshot.revision,
                        self.snapshot.room.as_ref().map(|room| room.phase),
                        self.snapshot
                            .room
                            .as_ref()
                            .and_then(|room| room.deadline_ns),
                        members,
                    )?)
                };
                let error = self
                    .ui_error
                    .as_deref()
                    .or(self.hud_error.as_deref())
                    .or_else(|| self.failure.as_ref().map(|error| error.message.as_str()))
                    .or_else(|| {
                        self.snapshot.terminal.as_ref().and_then(|outcome| {
                            outcome
                                .cleanup_error
                                .as_ref()
                                .map(|error| error.message.as_str())
                        })
                    })
                    .map(ui_message);
                Ok(Arc::new(RoomPresentation::new(
                    lobby,
                    status,
                    self.hud.as_ref(),
                    error,
                )?))
            })();
            match build {
                Ok(page) => {
                    self.ui_presentation = Some(page.clone());
                    self.ui_dirty = false;
                    if let Err(error) = self.host.publish(page) {
                        self.ui_error.get_or_insert(error);
                        self.ui_dirty = true;
                    }
                }
                Err(error) => {
                    self.ui_error.get_or_insert(error);
                }
            }
        } else {
            self.host.retry_publication();
        }
    }

    fn validated_local(&self, members: &[MemberProgress]) -> io::Result<Vec<MemberProgress>> {
        validate_members(self.local.as_deref(), members)
            .map_err(|error| invalid(error.to_string()))?;
        if members.len() != self.players.len()
            || members
                .iter()
                .zip(&self.players)
                .any(|(member, player)| member.player != *player)
        {
            return Err(invalid(
                "native room progress changed the actual player order",
            ));
        }
        copy_members(members)
    }

    /// Only invalid local input/allocation is a gameplay error. Peer/network
    /// failure disables comparison, preserving this actual local prefix.
    pub fn observe(&mut self, members: &[MemberProgress]) -> io::Result<()> {
        if self.finishing || self.outcome.is_some() {
            return Err(invalid("native room competition is finished"));
        }
        self.local = Some(self.validated_local(members)?);
        let _ = self.poll();
        if self.failure.is_some()
            || self.snapshot.terminal.is_some()
            || self.snapshot.schedule.is_none()
            || self.leaving
            || self.cancelled
            || self.pending(CommandKind::Leave)
        {
            return Ok(());
        }
        let now = match self.clock() {
            Ok(now) => now,
            Err(error) => {
                self.fail_network(error);
                return Ok(());
            }
        };
        if !self.publication.observe(now as u64) {
            self.fail_network(io::Error::new(
                io::ErrorKind::InvalidData,
                "native room publication clock regressed",
            ));
            return Ok(());
        }
        if !self.publication.due(false) {
            return Ok(());
        }
        let publication = copy_members(members)?;
        match self.send(
            CommandKind::Progress,
            RoomCommand::Publish {
                members: publication,
                final_prefix: false,
            },
        ) {
            Ok(_) => self.publication.admitted(),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => self.fail_network(error),
        }
        Ok(())
    }

    fn finish_naturally(&mut self, members: Vec<MemberProgress>) -> io::Result<()> {
        let started = self.clock()?;
        let duration = i64::try_from(self.finish_timeout.as_nanos())
            .map_err(|_| invalid("native room finish timeout overflow"))?;
        let deadline_ns = started
            .checked_add(duration)
            .ok_or_else(|| invalid("native room finish deadline overflow"))?;
        let (mut control, control_deadline_ns) = self.runtime.final_wait(self.finish_timeout)?;
        crate::room_final_wait::wait_for_room_final(
            &mut RoomFinalAdapter {
                owner: self,
                members: &members,
            },
            &mut control,
            deadline_ns,
            control_deadline_ns,
        )
        .map_err(|error| match error {
            crate::room_final_wait::RoomFinalWaitError::Port(error)
            | crate::room_final_wait::RoomFinalWaitError::Control(error) => error,
            crate::room_final_wait::RoomFinalWaitError::TimedOut => io::Error::new(
                io::ErrorKind::TimedOut,
                "native room final/drain/cleanup deadline expired",
            ),
            crate::room_final_wait::RoomFinalWaitError::InvalidTerminal => io::Error::new(
                io::ErrorKind::InvalidData,
                "native room ended without successful coordinated drain",
            ),
            crate::room_final_wait::RoomFinalWaitError::InvalidClock => {
                invalid("native room finish clock is negative")
            }
            crate::room_final_wait::RoomFinalWaitError::ClockRegressed => {
                invalid("native room finish clock regressed")
            }
        })
    }

    /// Call after genuine local completion and device teardown. Cancellation or
    /// an unplayed setup always joins without manufacturing a terminal prefix.
    /// A historical Complete with timeout/cleanup failure is not successful finish.
    pub fn finish(&mut self, members: &[MemberProgress], completed: bool) -> RoomOutcome {
        if let Some(outcome) = &self.outcome {
            return outcome.clone();
        }
        self.finishing = true;
        self.host.close_controls();
        let _ = self.poll();
        let result = if completed && self.local.is_some() {
            self.validated_local(members).and_then(|terminal| {
                self.local = Some(copy_members(&terminal)?);
                if self.snapshot.schedule.is_none()
                    || self.failure.is_some()
                    || self.leaving
                    || self.cancelled
                    || self.pending(CommandKind::Leave)
                {
                    return Err(io::Error::new(
                        io::ErrorKind::NotConnected,
                        "natural room finish requires an observed committed live game",
                    ));
                }
                self.finish_naturally(terminal)
            })
        } else {
            self.cancelled = true;
            Ok(())
        };
        if let Err(error) = result {
            self.fail_network(error);
        }
        // Even successful natural finish joins the existing terminal thread;
        // stop is never used as a substitute for receiving its actual outcome.
        let mut outcome = self.port.stop();
        // Joining settles queued commands as well. Preserve their actual local
        // refusals and any final accepted peer prefix for the original caller.
        match self.port.poll() {
            Ok(poll) => {
                if let Err(error) = self.accept_poll_inner(poll, true) {
                    self.fail_network(error);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => self.fail_network(error),
        }
        if outcome.error.is_none() {
            outcome.error = self.failure.clone();
        }
        self.snapshot.receipts = outcome.receipts;
        self.snapshot.terminal = Some(outcome.clone());
        self.hud_status(RoomHudStatus::Disconnected);
        self.outcome = Some(outcome.clone());
        self.ui_dirty = true;
        self.publish_ui();
        self.publish_results(&outcome);
        outcome
    }

    fn publish_results(&mut self, outcome: &RoomOutcome) {
        if !self.host.attached() || self.prepared.is_none() {
            return;
        }
        let built = (|| -> Result<RoomResults, String> {
            let hud = self.hud.as_ref().ok_or("joined room HUD is unavailable")?;
            let lobby = self
                .ui_presentation
                .as_ref()
                .ok_or("joined room metadata is unavailable")?
                .lobby
                .clone();
            let mut diagnostics = Vec::new();
            for (label, message) in [
                (
                    "display",
                    self.ui_error.as_deref().or(self.hud_error.as_deref()),
                ),
                (
                    "network",
                    outcome.error.as_ref().map(|error| error.message.as_str()),
                ),
                (
                    "cleanup",
                    outcome
                        .cleanup_error
                        .as_ref()
                        .map(|error| error.message.as_str()),
                ),
            ] {
                if let Some(message) = message {
                    let bounded: String = message
                        .chars()
                        .take(256)
                        .map(|ch| if ch.is_control() { ' ' } else { ch })
                        .collect();
                    diagnostics.push(format!("{label}: {bounded}"));
                }
            }
            let error = if diagnostics.is_empty() {
                None
            } else {
                Some(diagnostics.join("; "))
            };
            RoomResults::new(lobby, hud, outcome.cancelled, error)
        })();
        let result = built.and_then(|archive| self.host.publish_results(Arc::new(archive)));
        if let Err(error) = result {
            // No mutation of failure/outcome: an archive is only presentation.
            self.ui_error.get_or_insert(error);
            self.ui_dirty = true;
            self.publish_ui();
        }
    }
}

struct RoomFinalAdapter<'a, P: RoomNetworkPort, H: RoomUiHost, R: RoomRuntimeHost> {
    owner: &'a mut RoomCompetition<P, H, R>,
    members: &'a [MemberProgress],
}
impl<P: RoomNetworkPort, H: RoomUiHost, R: RoomRuntimeHost> crate::room_final_wait::RoomFinalPort
    for RoomFinalAdapter<'_, P, H, R>
{
    type Error = io::Error;
    fn poll(&mut self) -> io::Result<crate::room_final_wait::RoomFinalObservation> {
        use crate::room_final_wait::{RoomFinalObservation, RoomFinalTerminal};
        self.owner.poll()?;
        Ok(RoomFinalObservation {
            progress_pending: self.owner.pending(CommandKind::Progress),
            final_accepted: self.owner.final_accepted,
            drain_accepted: self.owner.drain_accepted,
            terminal: self
                .owner
                .snapshot
                .terminal
                .as_ref()
                .map(|terminal| RoomFinalTerminal {
                    cancelled: terminal.cancelled,
                    failed: terminal.error.is_some(),
                    receipts: terminal.receipts,
                }),
        })
    }
    fn clock_now_ns(&mut self) -> io::Result<i64> {
        self.owner.clock()
    }
    fn queue_final(&mut self) -> io::Result<crate::room_final_wait::RoomFinalAdmission> {
        use crate::room_final_wait::RoomFinalAdmission;
        let now = self
            .owner
            .last_clock
            .ok_or_else(|| invalid("native room final publication requires a clock observation"))?;
        if !self.owner.publication.observe(now as u64) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "native room publication clock regressed",
            ));
        }
        match self.owner.send(
            CommandKind::Final,
            RoomCommand::Publish {
                members: copy_members(self.members)?,
                final_prefix: true,
            },
        ) {
            Ok(_) => {
                self.owner.publication.admitted();
                Ok(RoomFinalAdmission::Accepted)
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                Ok(RoomFinalAdmission::QueueFull)
            }
            Err(error) => Err(error),
        }
    }
    fn queue_drain(&mut self) -> io::Result<crate::room_final_wait::RoomFinalAdmission> {
        use crate::room_final_wait::RoomFinalAdmission;
        match self.owner.send(CommandKind::Drain, RoomCommand::Drain) {
            Ok(_) => Ok(RoomFinalAdmission::Accepted),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                Ok(RoomFinalAdmission::QueueFull)
            }
            Err(error) => Err(error),
        }
    }
}

struct RoomStartAdapter<'a, P: RoomNetworkPort, H: RoomUiHost, R: RoomRuntimeHost>(
    &'a mut RoomCompetition<P, H, R>,
);
impl<P: RoomNetworkPort, H: RoomUiHost, R: RoomRuntimeHost> crate::room_start_wait::RoomStartPort
    for RoomStartAdapter<'_, P, H, R>
{
    type Error = io::Error;
    fn initial(&mut self) -> crate::room_start_wait::RoomStartInitial {
        self.0.service_ui_cancellation();
        crate::room_start_wait::RoomStartInitial {
            cancelled: self.0.cancelled,
            closing: self.0.finishing
                || self.0.outcome.is_some()
                || self.0.cancelled
                || self.0.leaving
                || self.0.pending(CommandKind::Leave),
        }
    }
    fn poll(&mut self) -> crate::room_start_wait::RoomStartObservation<Self::Error> {
        let polled = self.0.poll();
        crate::room_start_wait::RoomStartObservation {
            cancelled: self.0.cancelled,
            failure: polled.err(),
            leaving: self.0.leaving || self.0.pending(CommandKind::Leave),
            terminal: self.0.snapshot.terminal.is_some(),
            committed: self.0.snapshot.schedule.is_some(),
        }
    }
}

impl<P: RoomNetworkPort, H: RoomUiHost, R: RoomRuntimeHost> RoomCompetition<P, H, R> {
    pub fn await_commit_with_service<F, S>(
        &mut self,
        service: &mut F,
    ) -> Result<bool, crate::room_start_wait::RoomStartWaitError<io::Error, S, io::Error>>
    where
        F: FnMut() -> Result<bool, S> + ?Sized,
        S: std::fmt::Display,
    {
        let mut control = self.runtime.start_wait();
        let result = crate::room_start_wait::await_room_start(
            &mut RoomStartAdapter(self),
            &mut control,
            service,
        );
        match &result {
            Ok(false) => {
                self.cancelled = true;
                self.port.request_stop();
                self.hud_status(RoomHudStatus::Disconnected);
            }
            Err(error) => {
                use crate::room_start_wait::RoomStartWaitError;
                let message = match error {
                    RoomStartWaitError::Port(error) | RoomStartWaitError::Control(error) => {
                        error.to_string()
                    }
                    RoomStartWaitError::Service(error) => error.to_string(),
                    RoomStartWaitError::Closing => "native room startup is closing".into(),
                    RoomStartWaitError::LeavePending => {
                        "native room Leave is pending before output activation".into()
                    }
                    RoomStartWaitError::Terminal => {
                        "native room ended before output activation".into()
                    }
                };
                self.fail_network(io::Error::other(message));
            }
            Ok(true) => {}
        }
        result
    }

    pub fn committed_schedule_value(&self) -> Result<StartSchedule, &'static str> {
        if self.failure.is_some()
            || self.snapshot.terminal.is_some()
            || self.cancelled
            || self.leaving
            || self.pending(CommandKind::Leave)
            || self.outcome.is_some()
        {
            return Err("native room committed schedule is no longer active");
        }
        self.snapshot
            .schedule
            .ok_or("native room committed schedule is missing")
    }
    pub fn room_clock_ns(&self) -> io::Result<i64> {
        self.port.clock_now_ns()
    }
}

impl<P: RoomNetworkPort, H: RoomUiHost, R: RoomRuntimeHost> Drop for RoomCompetition<P, H, R> {
    fn drop(&mut self) {
        if self.outcome.is_none() {
            self.port.request_stop();
            let outcome = self.port.stop();
            self.runtime.dropped(&outcome);
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod fixtures {
    include!("native_room_competition_fixtures.rs");
    mod results {
        include!("native_room_results_fixtures.rs");
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "native_room_ui_fixtures.rs"]
mod ui_fixtures;

#[cfg(all(test, not(target_arch = "wasm32")))]
use crate::player;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "room_ui_host_fixtures.rs"]
mod room_ui_host_fixtures;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "room_runtime_host_fixtures.rs"]
mod room_runtime_host_fixtures;

#[cfg(all(test, not(target_arch = "wasm32")))]
use crate::{
    native_room_competition::{NativeRoomCompetition, NativeRoomPort},
    native_room_network::{
        NativeRoomCommand, NativeRoomFailure, NativeRoomOutcome, NativeRoomPoll, NativeRoomReply,
        NativeRoomRoster, NativeRoomSnapshot,
    },
    native_start::NativeStartAgreement,
};

#[cfg(test)]
#[path = "room_competition_fixtures.rs"]
mod room_competition_fixtures;

#[cfg(test)]
#[path = "room_controller_publication_fixtures.rs"]
mod room_controller_publication_fixtures;
