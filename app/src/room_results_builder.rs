//! Bounded reconstruction of joined room presentation from admitted metadata.
use std::sync::Arc;

use crate::{
    local_players::PlayerId,
    multiplayer_group::{GroupPrefix, decode_words},
    multiplayer_group_rooms::{GroupRoomMember, GroupRoomPhase},
    multiplayer_room_wire::validate_snapshot,
    multiplayer_rooms::ParticipantId,
    room_opponent_hud::RoomOpponentHud,
    room_presentation::{RoomLobby, RoomPresentation, RoomResults},
};

/// Prepared hosts in their original order: [host low, host high, count, players...].
pub fn decode_room_roster(words: &[u32]) -> Result<Vec<GroupRoomMember>, String> {
    if words.is_empty() || words.len() > 4288 {
        return Err("room roster exceeds its bounded word layout".into());
    }
    let mut members = Vec::new();
    members
        .try_reserve_exact(64)
        .map_err(|_| "room roster allocation failed")?;
    let mut offset = 0;
    while offset < words.len() {
        let header = words
            .get(offset..offset + 3)
            .ok_or("incomplete room host")?;
        let count = header[2] as usize;
        if !(1..=64).contains(&count) || members.len() == 64 {
            return Err("room roster exceeds its host/player bounds".into());
        }
        offset += 3;
        let source = words
            .get(offset..offset + count)
            .ok_or("incomplete room players")?;
        let mut players = Vec::new();
        players
            .try_reserve_exact(count)
            .map_err(|_| "room player allocation failed")?;
        players.extend(source.iter().map(|&player| PlayerId(player)));
        members.push(GroupRoomMember {
            id: ParticipantId(u64::from(header[0]) | (u64::from(header[1]) << 32)),
            players,
            prepared: true,
        });
        offset += count;
    }
    validate_snapshot(&members, GroupRoomPhase::Prepared, None)
        .map_err(|error| error.to_string())?;
    Ok(members)
}

/// The caller supplies only genuinely accepted prefixes after joining its room
/// owner. This model has no transport, completion or acknowledgement authority.
#[derive(Debug)]
pub struct RoomResultsBuilder {
    lobby: Arc<RoomLobby>,
    hud: Option<RoomOpponentHud>,
    archive: Option<Arc<RoomResults>>,
    presentation: Option<RoomPresentation>,
}
impl RoomResultsBuilder {
    pub fn new(own: ParticipantId, roster_words: &[u32]) -> Result<Self, String> {
        let members = decode_room_roster(roster_words)?;
        let hud = RoomOpponentHud::new(own, &members)?;
        let lobby = Arc::new(RoomLobby::new(
            Some(own),
            1,
            Some(GroupRoomPhase::Prepared),
            None,
            members,
        )?);
        Ok(Self {
            lobby,
            hud: Some(hud),
            archive: None,
            presentation: None,
        })
    }
    pub fn update(
        &mut self,
        participant: ParticipantId,
        sequence: u64,
        final_prefix: bool,
        words: &[u32],
    ) -> Result<(), String> {
        let hud = self.hud.as_mut().ok_or("room Results are already frozen")?;
        let members = decode_words(words).map_err(|error| error.to_string())?;
        hud.update(
            participant,
            &GroupPrefix {
                sequence,
                final_prefix,
                members,
            },
        )
    }
    pub fn freeze(
        &mut self,
        initial_page: usize,
        cancelled: bool,
        error: Option<String>,
        failed: bool,
    ) -> Result<(), String> {
        let hud = self.hud.as_mut().ok_or("room Results are already frozen")?;
        if initial_page >= hud.page_count()
            || error
                .as_ref()
                .is_some_and(|text| text.len() > 4096 || text.chars().any(char::is_control))
        {
            return Err("invalid room Results page or diagnostic".into());
        }
        // Only this one-time freeze copies the full retained presentation. Page
        // selection below projects at most four cached rows from the same Arc.
        let mut candidate = hud.clone();
        candidate.set_page(initial_page)?;
        if failed {
            candidate.mark_failed();
        }
        let archive = Arc::new(RoomResults::new(
            self.lobby.clone(),
            &candidate,
            cancelled,
            error,
        )?);
        let presentation = archive.project(initial_page)?;
        self.archive = Some(archive);
        self.presentation = Some(presentation);
        self.hud = None;
        Ok(())
    }
    pub fn archive(&self) -> Option<&Arc<RoomResults>> {
        self.archive.as_ref()
    }
    pub fn presentation(&self) -> Option<&RoomPresentation> {
        self.presentation.as_ref()
    }
    pub fn set_page(&mut self, page: usize) -> Result<(), String> {
        let archive = self
            .archive
            .as_ref()
            .ok_or("freeze room Results before selecting a page")?;
        let presentation = archive.project(page)?;
        self.presentation = Some(presentation);
        Ok(())
    }
    pub fn pages(&self) -> usize {
        self.archive
            .as_ref()
            .map_or(0, |archive| archive.page_count())
    }
    pub fn failed(&self) -> bool {
        self.archive
            .as_ref()
            .is_some_and(|archive| archive.failed())
    }
}

/// Frozen checked page projections of actual joined-room results, never ranking evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenRoomResults {
    pub pages: Vec<RoomPresentation>,
    pub initial_page: usize,
}
impl FrozenRoomResults {
    pub fn from_archive(archive: &RoomResults) -> Result<Self, String> {
        let mut pages = Vec::new();
        pages
            .try_reserve_exact(archive.page_count())
            .map_err(|error| error.to_string())?;
        for page in 0..archive.page_count() {
            pages.push(archive.project(page)?);
        }
        let model = Self {
            pages,
            initial_page: archive.initial_page(),
        };
        model.validate()?;
        Ok(model)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.pages.is_empty() || self.pages.len() > 1008 || self.initial_page >= self.pages.len()
        {
            return Err("frozen room page capacity exceeded".into());
        }
        let first = &self.pages[0];
        for (index, page) in self.pages.iter().enumerate() {
            page.validate()?;
            if page.page != index
                || page.pages != self.pages.len()
                || page.status != crate::room_presentation::RoomStatus::Closed
                || page.lobby != first.lobby
                || page.failed != first.failed
                || page.error != first.error
            {
                return Err("frozen room pages differ from actual shared registration".into());
            }
        }
        Ok(())
    }
    pub fn project(&self, page: usize) -> Result<&RoomPresentation, String> {
        self.validate()?;
        self.pages
            .get(page)
            .ok_or_else(|| "frozen room page out of range".into())
    }
    /// Shared lobby encoded once; up to4032 rows with three bounded128-byte labels.
    pub fn encoded_bytes(&self) -> Result<usize, String> {
        self.validate()?;
        let lobby = &self.pages[0].lobby;
        Ok(128
            + lobby
                .members
                .iter()
                .map(|member| 32 + member.players.len() * 4)
                .sum::<usize>()
            + self
                .pages
                .iter()
                .map(|page| {
                    64 + page.heading.len()
                        + page.error.as_ref().map_or(0, String::len)
                        + page
                            .rows
                            .iter()
                            .map(|row| {
                                64 + row.label.len()
                                    + row.counters.iter().map(String::len).sum::<usize>()
                            })
                            .sum::<usize>()
                })
                .sum::<usize>())
    }
}
impl RoomResultsBuilder {
    pub fn export_visual(&self) -> Result<Option<FrozenRoomResults>, String> {
        self.archive
            .as_ref()
            .map(|archive| {
                let mut model = FrozenRoomResults::from_archive(archive)?;
                model.initial_page = self
                    .presentation
                    .as_ref()
                    .ok_or("frozen room current page missing")?
                    .page;
                model.validate()?;
                Ok(model)
            })
            .transpose()
    }
}
