//! Retained, participant-scoped reported scores; never advances local judgment.
use crate::{
    local_players::PlayerId,
    multiplayer_group::{GroupPrefix, validate_members},
    multiplayer_group_rooms::{GroupRoomMember, GroupRoomPhase},
    multiplayer_protocol::{Progress, validate_progress},
    multiplayer_room_wire::validate_snapshot,
    multiplayer_rooms::ParticipantId,
};

pub const ROOM_SCORES_PER_PAGE: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomHudStatus {
    Waiting,
    Connected,
    Disconnected,
}

/// One remote player, always qualified by the real participant identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomOpponentRow {
    pub participant: ParticipantId,
    pub player: PlayerId,
    pub progress: Option<Progress>,
    pub final_prefix: bool,
    label: String,
    counters: [String; 2],
}

impl RoomOpponentRow {
    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn counters(&self) -> &[String; 2] {
        &self.counters
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HostRows {
    participant: ParticipantId,
    start: usize,
    count: usize,
    sequence: Option<u64>,
    final_prefix: bool,
}

/// Frozen host/player order with cached page text. Rendering borrows at most
/// four entries, without formatting or scanning the complete remote roster.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomOpponentHud {
    hosts: Vec<HostRows>,
    rows: Vec<RoomOpponentRow>,
    page: usize,
    status: RoomHudStatus,
    heading: String,
    failed: bool,
}

impl RoomOpponentHud {
    pub fn new(own: ParticipantId, members: &[GroupRoomMember]) -> Result<Self, String> {
        validate_snapshot(members, GroupRoomPhase::Prepared, None)
            .map_err(|error| error.to_string())?;
        if !members.iter().any(|member| member.id == own) {
            return Err("room HUD requires the actual admitted participant".into());
        }
        let count = members
            .iter()
            .filter(|member| member.id != own)
            .map(|member| member.players.len())
            .sum();
        let mut hosts = Vec::new();
        hosts
            .try_reserve_exact(members.len() - 1)
            .map_err(|_| "room HUD host allocation failed")?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(count)
            .map_err(|_| "room HUD row allocation failed")?;
        for member in members.iter().filter(|member| member.id != own) {
            hosts.push(HostRows {
                participant: member.id,
                start: rows.len(),
                count: member.players.len(),
                sequence: None,
                final_prefix: false,
            });
            for &player in &member.players {
                rows.push(RoomOpponentRow {
                    participant: member.id,
                    player,
                    progress: None,
                    final_prefix: false,
                    label: format!("HOST {} PLAYER {}", member.id.0, player.0),
                    counters: ["H -  M -".into(), "C -  MAX -".into()],
                });
            }
        }
        let mut hud = Self {
            hosts,
            rows,
            page: 0,
            status: RoomHudStatus::Waiting,
            heading: String::new(),
            failed: false,
        };
        hud.refresh_heading();
        Ok(hud)
    }

    pub fn update(
        &mut self,
        participant: ParticipantId,
        prefix: &GroupPrefix,
    ) -> Result<(), String> {
        self.update_prefix(participant, prefix, false)
    }

    /// The shared controller calls this after joining and reading its retained
    /// accepted prefixes. Disconnection still fences ordinary live updates.
    pub(crate) fn retain_after_join(
        &mut self,
        participant: ParticipantId,
        prefix: &GroupPrefix,
    ) -> Result<(), String> {
        self.update_prefix(participant, prefix, true)
    }

    fn update_prefix(
        &mut self,
        participant: ParticipantId,
        prefix: &GroupPrefix,
        joined: bool,
    ) -> Result<(), String> {
        self.ensure_live()?;
        if !joined && self.status == RoomHudStatus::Disconnected {
            return Err("room HUD is disconnected".into());
        }
        let index = self
            .hosts
            .iter()
            .position(|host| host.participant == participant)
            .ok_or("unknown room HUD participant")?;
        let host = &self.hosts[index];
        if prefix.sequence == 0
            || host
                .sequence
                .is_some_and(|sequence| prefix.sequence <= sequence)
            || host.final_prefix
            || prefix.members.len() != host.count
        {
            return Err("invalid room HUD prefix identity or finality".into());
        }
        validate_members(None, &prefix.members).map_err(|error| error.to_string())?;
        let previous = &self.rows[host.start..host.start + host.count];
        for (row, member) in previous.iter().zip(&prefix.members) {
            if row.player != member.player {
                return Err("room HUD changed its prepared player order".into());
            }
            validate_progress(row.progress, member.progress).map_err(|error| error.to_string())?;
        }
        let mut candidate = Vec::new();
        candidate
            .try_reserve_exact(host.count)
            .map_err(|_| "room HUD prefix allocation failed")?;
        for member in &prefix.members {
            let progress = member.progress;
            candidate.push(RoomOpponentRow {
                participant,
                player: member.player,
                progress: Some(progress),
                final_prefix: prefix.final_prefix,
                label: format!(
                    "HOST {} PLAYER {}{}",
                    participant.0,
                    member.player.0,
                    if prefix.final_prefix { " FINAL" } else { "" }
                ),
                counters: [
                    format!("H {}  M {}", progress.hits, progress.misses),
                    format!("C {}  MAX {}", progress.combo, progress.max_combo),
                ],
            });
        }
        for (target, row) in self.rows[host.start..host.start + host.count]
            .iter_mut()
            .zip(candidate)
        {
            *target = row;
        }
        self.hosts[index].sequence = Some(prefix.sequence);
        self.hosts[index].final_prefix = prefix.final_prefix;
        Ok(())
    }

    pub fn set_status(&mut self, status: RoomHudStatus) -> Result<(), String> {
        self.ensure_live()?;
        if (self.status == RoomHudStatus::Disconnected && status != RoomHudStatus::Disconnected)
            || (self.status == RoomHudStatus::Connected && status == RoomHudStatus::Waiting)
        {
            return Err("room HUD connection status regressed".into());
        }
        if self.status != status {
            self.status = status;
            self.refresh_heading();
        }
        Ok(())
    }

    pub fn set_page(&mut self, page: usize) -> Result<(), String> {
        self.ensure_live()?;
        if page >= self.page_count() {
            return Err("room HUD page is out of range".into());
        }
        if self.page != page {
            self.page = page;
            self.refresh_heading();
        }
        Ok(())
    }
    pub fn page_index(&self) -> usize {
        self.page
    }
    pub fn page_count(&self) -> usize {
        self.rows.len().div_ceil(ROOM_SCORES_PER_PAGE)
    }
    pub fn entry_count(&self) -> usize {
        self.rows.len()
    }
    /// Full retained history for a one-time joined Results archive. Live
    /// drawing uses page(), which remains bounded to four visible rows.
    pub fn rows(&self) -> &[RoomOpponentRow] {
        &self.rows
    }
    pub fn page(&self) -> &[RoomOpponentRow] {
        if self.failed {
            return &[];
        }
        let start = self.page * ROOM_SCORES_PER_PAGE;
        &self.rows[start..(start + ROOM_SCORES_PER_PAGE).min(self.rows.len())]
    }
    pub fn heading(&self) -> &str {
        &self.heading
    }
    pub fn failed(&self) -> bool {
        self.failed
    }
    pub fn mark_failed(&mut self) {
        self.failed = true;
        self.heading = "ROOM SCORES UNAVAILABLE".into();
    }
    fn ensure_live(&self) -> Result<(), String> {
        if self.failed {
            Err("room HUD is disabled".into())
        } else {
            Ok(())
        }
    }
    fn refresh_heading(&mut self) {
        let status = match self.status {
            RoomHudStatus::Waiting => "WAITING",
            RoomHudStatus::Connected => "CONNECTED",
            RoomHudStatus::Disconnected => "DISCONNECTED",
        };
        self.heading = format!(
            "ROOM REPORTED SCORES - {} - PAGE {}/{}",
            status,
            self.page + 1,
            self.page_count()
        );
    }
}
