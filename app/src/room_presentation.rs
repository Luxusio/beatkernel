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

fn validate_row(row: &RoomScoreRow, identity: (ParticipantId, PlayerId)) -> Result<(), String> {
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
    Ok(())
}

/// Immutable joined-room history, shared through Arc. All remote identities
/// and cached text are copied once; page projection copies at most four rows.
#[derive(Debug, PartialEq, Eq)]
pub struct RoomResults {
    lobby: Arc<RoomLobby>,
    rows: Vec<RoomScoreRow>,
    initial_page: usize,
    cancelled: bool,
    failed: bool,
    error: Option<String>,
}
impl RoomResults {
    pub fn new(
        lobby: Arc<RoomLobby>,
        hud: &RoomOpponentHud,
        cancelled: bool,
        error: Option<String>,
    ) -> Result<Self, String> {
        lobby.validate()?;
        if lobby.phase != Some(GroupRoomPhase::Prepared)
            || error
                .as_ref()
                .is_some_and(|text| text.len() > 4096 || text.chars().any(char::is_control))
        {
            return Err("room Results require bounded prepared metadata".into());
        }
        let expected = lobby
            .members
            .iter()
            .filter(|host| Some(host.id) != lobby.participant)
            .flat_map(|host| host.players.iter().map(move |player| (host.id, *player)));
        let count = expected.clone().count();
        if count == 0
            || count > 4032
            || hud.rows().len() != count
            || hud.page_index() >= count.div_ceil(4)
        {
            return Err("room Results differ from the prepared roster".into());
        }
        let mut rows = Vec::new();
        rows.try_reserve_exact(count)
            .map_err(|_| "room Results allocation failed")?;
        for (row, identity) in hud.rows().iter().zip(expected) {
            let row = RoomScoreRow {
                participant: row.participant,
                player: row.player,
                progress: row.progress,
                final_prefix: row.final_prefix,
                label: row.label().to_owned(),
                counters: row.counters().clone(),
            };
            validate_row(&row, identity)?;
            rows.push(row);
        }
        Ok(Self {
            lobby,
            rows,
            initial_page: hud.page_index(),
            cancelled,
            failed: hud.failed(),
            error,
        })
    }
    pub fn lobby(&self) -> &Arc<RoomLobby> {
        &self.lobby
    }
    pub fn rows(&self) -> &[RoomScoreRow] {
        &self.rows
    }
    pub fn initial_page(&self) -> usize {
        self.initial_page
    }
    pub fn page_count(&self) -> usize {
        self.rows.len().div_ceil(4)
    }
    pub fn cancelled(&self) -> bool {
        self.cancelled
    }
    pub fn failed(&self) -> bool {
        self.failed
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Event-driven local selection. This never reopens any live room control.
    pub fn project(&self, page: usize) -> Result<RoomPresentation, String> {
        if page >= self.page_count() {
            return Err("room Results page is out of range".into());
        }
        let mut rows = Vec::new();
        if !self.failed {
            let start = page * 4;
            let selected = &self.rows[start..(start + 4).min(self.rows.len())];
            rows.try_reserve_exact(selected.len())
                .map_err(|_| "room Results page allocation failed")?;
            rows.extend_from_slice(selected);
        }
        Ok(RoomPresentation {
            lobby: self.lobby.clone(),
            status: RoomStatus::Closed,
            page,
            pages: self.page_count(),
            rows,
            heading: if self.failed {
                "ROOM SCORES UNAVAILABLE".into()
            } else if self.cancelled {
                format!(
                    "ROOM REPORTED RESULTS - CANCELLED - {}/{}",
                    page + 1,
                    self.page_count()
                )
            } else {
                format!(
                    "ROOM REPORTED RESULTS - PAGE {}/{}",
                    page + 1,
                    self.page_count()
                )
            },
            error: self.error.clone(),
            failed: self.failed,
        })
    }
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
            validate_row(row, identity)?;
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

#[cfg(test)]
#[path = "room_results_fixtures.rs"]
mod results_fixtures;
