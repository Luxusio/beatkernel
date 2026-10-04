//! Game-owned room comparison and output-start adaptation. Networking remains
//! on NativeRoomNetwork's thread; reported peer scores never enter a judge.

use crate::{
    local_players::PlayerId,
    multiplayer_group::{validate_members, validate_roster, MemberProgress},
    multiplayer_group_rooms::GroupRoomPhase,
    multiplayer_start::StartSchedule,
    native_room_network::{
        NativeRoomCommand, NativeRoomFailure, NativeRoomNetwork, NativeRoomOutcome, NativeRoomPoll,
        NativeRoomReply, NativeRoomRoster, NativeRoomSnapshot,
    },
    native_start::{NativeStartAgreement, NativeStartResult, SessionHostBracket},
    room_opponent_hud::{RoomHudStatus, RoomOpponentHud},
};
use beatkernel::time::ClockPoint;
use std::{
    collections::VecDeque,
    io,
    sync::Arc,
    time::{Duration, Instant},
};

const PUBLICATION_NS: i64 = 50_000_000;
const LOBBY_LIMIT: usize = 16;
const WAIT: Duration = Duration::from_millis(1);

/// The production implementation delegates only to the existing network owner.
/// In-memory ports can exercise this same controller without creating a second
/// admission, clock, progress or drain protocol.
pub trait NativeRoomPort {
    fn try_command(&mut self, command: NativeRoomCommand) -> io::Result<u64>;
    fn poll(&self) -> io::Result<NativeRoomPoll>;
    fn clock_now_ns(&self) -> io::Result<i64>;
    fn request_stop(&self);
    fn stop(&mut self) -> NativeRoomOutcome;
}
impl NativeRoomPort for NativeRoomNetwork {
    fn try_command(&mut self, command: NativeRoomCommand) -> io::Result<u64> {
        NativeRoomNetwork::try_command(self, command)
    }
    fn poll(&self) -> io::Result<NativeRoomPoll> {
        NativeRoomNetwork::poll(self)
    }
    fn clock_now_ns(&self) -> io::Result<i64> {
        NativeRoomNetwork::clock_now_ns(self)
    }
    fn request_stop(&self) {
        NativeRoomNetwork::request_stop(self);
    }
    fn stop(&mut self) -> NativeRoomOutcome {
        NativeRoomNetwork::stop(self)
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
fn from_failure(error: &NativeRoomFailure) -> io::Error {
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

/// Owns one actual local roster, one portable retained HUD and bounded command
/// correlations. A room snapshot alone never authorizes native audio startup.
pub struct NativeRoomCompetition<P: NativeRoomPort = NativeRoomNetwork> {
    port: P,
    players: Vec<PlayerId>,
    snapshot: NativeRoomSnapshot,
    prepared: Option<Arc<NativeRoomRoster>>,
    hud: Option<RoomOpponentHud>,
    hud_error: Option<String>,
    peer_sequences: Vec<(crate::multiplayer_rooms::ParticipantId, u64)>,
    local: Option<Vec<MemberProgress>>,
    pending: Vec<Pending>,
    replies: VecDeque<NativeRoomReply>,
    last_id: Option<u64>,
    last_clock: Option<i64>,
    last_publish: Option<i64>,
    failure: Option<NativeRoomFailure>,
    finish_timeout: Duration,
    finishing: bool,
    final_accepted: bool,
    drain_accepted: bool,
    leaving: bool,
    cancelled: bool,
    outcome: Option<NativeRoomOutcome>,
}

impl<P: NativeRoomPort> NativeRoomCompetition<P> {
    pub fn new(mut port: P, players: Vec<PlayerId>, finish_timeout: Duration) -> io::Result<Self> {
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
            Ok((pending, replies, peer_sequences))
        })();
        let (pending, replies, peer_sequences) = match validated {
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
            port,
            players,
            snapshot: NativeRoomSnapshot::default(),
            prepared: None,
            hud: None,
            hud_error: None,
            peer_sequences,
            local: None,
            pending,
            replies,
            last_id: None,
            last_clock: None,
            last_publish: None,
            failure: None,
            finish_timeout,
            finishing: false,
            final_accepted: false,
            drain_accepted: false,
            leaving: false,
            cancelled: false,
            outcome: None,
        })
    }

    pub fn players(&self) -> &[PlayerId] {
        &self.players
    }
    pub fn snapshot(&self) -> &NativeRoomSnapshot {
        &self.snapshot
    }
    pub fn local_progress(&self) -> Option<&[MemberProgress]> {
        self.local.as_deref()
    }
    pub fn hud(&self) -> Option<&RoomOpponentHud> {
        self.hud.as_ref()
    }
    pub fn hud_error(&self) -> Option<&str> {
        self.hud_error.as_deref()
    }
    pub fn network_error(&self) -> Option<&NativeRoomFailure> {
        self.failure.as_ref()
    }
    pub fn outcome(&self) -> Option<&NativeRoomOutcome> {
        self.outcome.as_ref()
    }
    pub fn take_reply(&mut self) -> Option<NativeRoomReply> {
        self.replies.pop_front()
    }

    pub fn set_page(&mut self, page: usize) -> Result<(), String> {
        self.hud
            .as_mut()
            .ok_or("room HUD is not prepared")?
            .set_page(page)
    }
    pub fn disable_hud(&mut self) {
        self.fail_hud("room score presentation was disabled".into());
    }
    fn fail_hud(&mut self, error: String) {
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
                }
            }
        }
    }
    fn fail_network(&mut self, error: io::Error) {
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

    fn send(&mut self, kind: CommandKind, command: NativeRoomCommand) -> io::Result<u64> {
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
    fn lobby(&mut self, kind: CommandKind, command: NativeRoomCommand) -> io::Result<u64> {
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
        self.lobby(CommandKind::Seal, NativeRoomCommand::Seal)
    }
    pub fn ready(&mut self) -> io::Result<u64> {
        self.lobby(CommandKind::Ready, NativeRoomCommand::Ready)
    }
    pub fn leave(&mut self) -> io::Result<u64> {
        self.lobby(CommandKind::Leave, NativeRoomCommand::Leave)
    }

    fn accept_poll(&mut self, poll: NativeRoomPoll) -> io::Result<()> {
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
        if self.hud_error.is_none() && self.failure.is_none() {
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
                if let Err(error) = hud.update(*participant, prefix) {
                    self.hud_error = Some(error);
                    hud.mark_failed();
                    break;
                }
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
            || self.pending(CommandKind::Progress)
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
        if self
            .last_publish
            .is_some_and(|previous| now - previous < PUBLICATION_NS)
        {
            return Ok(());
        }
        let publication = copy_members(members)?;
        match self.send(
            CommandKind::Progress,
            NativeRoomCommand::Publish {
                members: publication,
                final_prefix: false,
            },
        ) {
            Ok(_) => self.last_publish = Some(now),
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
        let deadline = Instant::now()
            .checked_add(self.finish_timeout)
            .ok_or_else(|| invalid("native room finish deadline overflow"))?;
        let mut final_queued = false;
        let mut drain_queued = false;
        loop {
            self.poll()?;
            let now = self.clock()?;
            if now >= deadline_ns || Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "native room final/drain/cleanup deadline expired",
                ));
            }
            if let Some(terminal) = &self.snapshot.terminal {
                let receipts = terminal.receipts;
                if !terminal.cancelled
                    && terminal.error.is_none()
                    && final_queued
                    && self.final_accepted
                    && drain_queued
                    && self.drain_accepted
                    && receipts.local_final_written
                    && receipts.local_final_acknowledged
                    && receipts.progress_complete
                    && receipts.drain_complete
                {
                    // Waiting is over, not a success claim: finish returns the
                    // real terminal cleanup_error separately after joining.
                    return Ok(());
                }
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "native room ended without successful coordinated drain",
                ));
            }
            if !final_queued && !self.pending(CommandKind::Progress) {
                match self.send(
                    CommandKind::Final,
                    NativeRoomCommand::Publish {
                        members: copy_members(&members)?,
                        final_prefix: true,
                    },
                ) {
                    Ok(_) => final_queued = true,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(error),
                }
            }
            if self.final_accepted && !drain_queued {
                match self.send(CommandKind::Drain, NativeRoomCommand::Drain) {
                    Ok(_) => drain_queued = true,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(error),
                }
            }
            std::thread::sleep(WAIT.min(deadline.saturating_duration_since(Instant::now())));
        }
    }

    /// Call after genuine local completion and device teardown. Cancellation or
    /// an unplayed setup always joins without manufacturing a terminal prefix.
    /// A historical Complete with timeout/cleanup failure is not successful finish.
    pub fn finish(&mut self, members: &[MemberProgress], completed: bool) -> NativeRoomOutcome {
        if let Some(outcome) = &self.outcome {
            return outcome.clone();
        }
        self.finishing = true;
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
                if let Err(error) = self.accept_poll(poll) {
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
        outcome
    }
}

impl<P: NativeRoomPort> NativeStartAgreement for NativeRoomCompetition<P> {
    fn await_commit(
        &mut self,
        service: &mut dyn FnMut() -> NativeStartResult<bool>,
    ) -> NativeStartResult<bool> {
        let result = (|| -> NativeStartResult<bool> {
            if self.finishing
                || self.outcome.is_some()
                || self.cancelled
                || self.leaving
                || self.pending(CommandKind::Leave)
            {
                return Err("native room startup is closing".into());
            }
            loop {
                if !service()? {
                    return Ok(false);
                }
                self.poll()?;
                // A late failed Commit can be retained as history; terminal
                // failure must be checked before using that schedule for output.
                if self.snapshot.terminal.is_some() {
                    return Err("native room ended before output activation".into());
                }
                if self.snapshot.schedule.is_some() {
                    return Ok(true);
                }
                std::thread::sleep(WAIT);
            }
        })();
        match &result {
            Ok(false) => {
                self.cancelled = true;
                self.port.request_stop();
                self.hud_status(RoomHudStatus::Disconnected);
            }
            Err(error) => self.fail_network(io::Error::other(error.to_string())),
            Ok(true) => {}
        }
        result
    }
    fn committed_schedule(&self) -> NativeStartResult<StartSchedule> {
        if self.failure.is_some()
            || self.snapshot.terminal.is_some()
            || self.cancelled
            || self.leaving
            || self.pending(CommandKind::Leave)
            || self.outcome.is_some()
        {
            return Err("native room committed schedule is no longer active".into());
        }
        self.snapshot
            .schedule
            .ok_or_else(|| "native room committed schedule is missing".into())
    }
    fn host_bracket(
        &self,
        sample: &mut dyn FnMut() -> NativeStartResult<ClockPoint>,
    ) -> NativeStartResult<SessionHostBracket> {
        self.committed_schedule()?;
        let before = self.port.clock_now_ns()?;
        let host = sample()?;
        let after = self.port.clock_now_ns()?;
        Ok(SessionHostBracket::new(before, host, after)?)
    }
    fn clock_now_ns(&self) -> NativeStartResult<i64> {
        Ok(self.port.clock_now_ns()?)
    }
}

impl<P: NativeRoomPort> Drop for NativeRoomCompetition<P> {
    fn drop(&mut self) {
        if self.outcome.is_none() {
            self.port.request_stop();
            let outcome = self.port.stop();
            if let Some(error) = outcome.cleanup_error.as_ref().or(outcome.error.as_ref()) {
                eprintln!("native room competition dropped after failure: {error}");
            }
        }
    }
}

#[cfg(test)]
#[path = "native_room_competition_fixtures.rs"]
mod fixtures;
