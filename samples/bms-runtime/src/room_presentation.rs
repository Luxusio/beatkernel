//! Portable room UI data and intent. No transport, native owner or judge authority.
use crate::{
    local_players::PlayerId,
    multiplayer_group_rooms::{GroupRoomMember, GroupRoomPhase},
    multiplayer_protocol::{Progress, validate_progress},
    multiplayer_room_wire::validate_snapshot,
    multiplayer_rooms::ParticipantId,
    room_opponent_hud::RoomOpponentHud,
};
use std::sync::Arc;

pub const ROOM_UI_CAPACITY: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomUiAction {
    Seal,
    Ready,
    Leave,
    Page(usize),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoomUiRequest {
    pub id: u64,
    pub action: RoomUiAction,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomUiReply {
    pub id: u64,
    pub result: Result<(), String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomStatus {
    Waiting,
    Connected,
    Disconnected,
    Closed,
}

/// Shared only when actual admission metadata changes; player frame copies keep
/// this Arc rather than copying every host's full player roster.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomLobby {
    pub participant: Option<ParticipantId>,
    pub revision: u64,
    pub phase: Option<GroupRoomPhase>,
    pub deadline_ns: Option<i64>,
    pub members: Vec<GroupRoomMember>,
}
impl RoomLobby {
    pub fn new(
        participant: Option<ParticipantId>,
        revision: u64,
        phase: Option<GroupRoomPhase>,
        deadline_ns: Option<i64>,
        members: Vec<GroupRoomMember>,
    ) -> Result<Self, String> {
        let value = Self {
            participant,
            revision,
            phase,
            deadline_ns,
            members,
        };
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> Result<(), String> {
        if self.participant.is_some_and(|id| id.0 == 0) {
            return Err("invalid room participant".into());
        }
        match self.phase {
            Some(phase) => {
                validate_snapshot(&self.members, phase, self.deadline_ns)
                    .map_err(|error| error.to_string())?;
                if self.revision == 0
                    || !self
                        .members
                        .iter()
                        .any(|member| Some(member.id) == self.participant)
                {
                    return Err("room metadata excludes the admitted participant".into());
                }
            }
            None if !self.members.is_empty() || self.deadline_ns.is_some() => {
                return Err("room membership requires an actual snapshot phase".into());
            }
            None => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomScoreRow {
    pub participant: ParticipantId,
    pub player: PlayerId,
    pub progress: Option<Progress>,
    pub final_prefix: bool,
    pub label: String,
    pub counters: [String; 2],
}

/// At most four already formatted rows. Building this never clones the full HUD.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomPresentation {
    pub lobby: Arc<RoomLobby>,
    pub status: RoomStatus,
    pub page: usize,
    pub pages: usize,
    pub heading: String,
    pub rows: Vec<RoomScoreRow>,
    pub error: Option<String>,
    pub failed: bool,
}
impl RoomPresentation {
    pub fn new(
        lobby: Arc<RoomLobby>,
        status: RoomStatus,
        hud: Option<&RoomOpponentHud>,
        error: Option<String>,
    ) -> Result<Self, String> {
        let mut rows = Vec::new();
        rows.try_reserve_exact(4)
            .map_err(|_| "room page allocation failed")?;
        if let Some(hud) = hud {
            for row in hud.page() {
                rows.push(RoomScoreRow {
                    participant: row.participant,
                    player: row.player,
                    progress: row.progress,
                    final_prefix: row.final_prefix,
                    label: row.label().to_owned(),
                    counters: row.counters().clone(),
                });
            }
        }
        let value = Self {
            lobby,
            status,
            page: hud.map_or(0, RoomOpponentHud::page_index),
            pages: hud.map_or(0, RoomOpponentHud::page_count),
            heading: hud.map_or_else(
                || {
                    match status {
                        RoomStatus::Waiting => "ROOM WAITING FOR MEMBERS",
                        RoomStatus::Connected => "ROOM CONNECTED",
                        RoomStatus::Disconnected => "ROOM DISCONNECTED",
                        RoomStatus::Closed => "ROOM CLOSED",
                    }
                    .to_owned()
                },
                |hud| hud.heading().to_owned(),
            ),
            rows,
            error,
            failed: hud.is_some_and(RoomOpponentHud::failed),
        };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), String> {
        self.lobby.validate()?;
        if self.rows.len() > 4
            || (self.failed && !self.rows.is_empty())
            || self.pages > 1008
            || (self.pages == 0 && (self.page != 0 || !self.rows.is_empty()))
            || (self.pages > 0 && self.page >= self.pages)
            || self.heading.len() > 256
            || self.heading.chars().any(char::is_control)
            || self
                .error
                .as_ref()
                .is_some_and(|error| error.len() > 4096 || error.chars().any(char::is_control))
        {
            return Err("room page exceeds presentation bounds".into());
        }
        let remote = self
            .lobby
            .members
            .iter()
            .filter(|member| Some(member.id) != self.lobby.participant);
        let count: usize = remote.clone().map(|member| member.players.len()).sum();
        if self.pages > 0
            && (self.lobby.phase != Some(GroupRoomPhase::Prepared)
                || self.pages != count.div_ceil(4))
        {
            return Err("room score pages differ from the prepared roster".into());
        }
        let expected = remote
            .flat_map(|member| {
                member
                    .players
                    .iter()
                    .map(move |player| (member.id, *player))
            })
            .skip(self.page * 4)
            .take(4);
        if self.pages > 0
            && !self.failed
            && self.rows.len() != count.saturating_sub(self.page * 4).min(4)
        {
            return Err("room score page is incomplete".into());
        }
        for (row, identity) in self.rows.iter().zip(expected) {
            if (row.participant, row.player) != identity
                || row.label.len() > 128
                || row.label.chars().any(char::is_control)
                || row
                    .counters
                    .iter()
                    .any(|text| text.len() > 128 || text.chars().any(char::is_control))
                || (row.final_prefix && row.progress.is_none())
            {
                return Err("room score row changed its actual host/player identity".into());
            }
            if let Some(progress) = row.progress {
                validate_progress(None, progress).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }
    /// A local UI enablement rule, never permission to mutate protocol state.
    pub fn allows(&self, action: RoomUiAction) -> bool {
        if self.status == RoomStatus::Closed {
            return false;
        }
        match action {
            RoomUiAction::Page(page) => !self.failed && self.pages > 0 && page < self.pages,
            RoomUiAction::Leave => {
                matches!(self.status, RoomStatus::Waiting | RoomStatus::Connected)
                    && self.lobby.participant.is_some()
            }
            RoomUiAction::Seal => {
                self.status == RoomStatus::Waiting
                    && self.lobby.phase == Some(GroupRoomPhase::Collecting)
                    && self.lobby.members.len() >= 2
                    && self
                        .lobby
                        .members
                        .first()
                        .is_some_and(|member| Some(member.id) == self.lobby.participant)
            }
            RoomUiAction::Ready => {
                self.status == RoomStatus::Waiting
                    && self.lobby.phase == Some(GroupRoomPhase::Frozen)
                    && self
                        .lobby
                        .members
                        .iter()
                        .any(|member| Some(member.id) == self.lobby.participant && !member.prepared)
            }
        }
    }
}
