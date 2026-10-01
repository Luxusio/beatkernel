//! Scalable local input ownership; native attachment resolution belongs to preparation.
use crate::settings::{MAX_VALUE_BYTES, SettingsHost};
use beatkernel::input::DeviceId;

pub const MAX_LOCAL_PLAYERS: usize = 64;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlayerId(pub u32);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalPlayer {
    pub id: PlayerId,
    input: Option<String>,
}
impl LocalPlayer {
    pub fn input(&self) -> Option<&str> {
        self.input.as_deref()
    }
}
#[derive(Clone, Debug)]
pub struct LocalPlayers {
    host: SettingsHost,
    capacity: usize,
    next_id: u32,
    players: Vec<LocalPlayer>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputPlan {
    Automatic { player: PlayerId },
    Assigned(Vec<(PlayerId, String)>),
}
impl LocalPlayers {
    pub fn new(host: SettingsHost, capacity: usize) -> Result<Self, String> {
        if !(1..=MAX_LOCAL_PLAYERS).contains(&capacity) {
            return Err("invalid local player capacity".into());
        }
        Ok(Self {
            host,
            capacity,
            next_id: 2,
            players: vec![LocalPlayer {
                id: PlayerId(1),
                input: None,
            }],
        })
    }
    pub fn players(&self) -> &[LocalPlayer] {
        &self.players
    }
    pub fn needs_assignment(&self) -> bool {
        self.players.len() > 1
    }
    /// Keep existing member IDs; retired IDs are never reused.
    pub fn resize(&mut self, count: usize) -> Result<(), String> {
        if !(1..=self.capacity).contains(&count) {
            return Err("local player count exceeds capacity".into());
        }
        let added = count.saturating_sub(self.players.len());
        let next = self
            .next_id
            .checked_add(u32::try_from(added).map_err(|_| "player count overflow")?)
            .ok_or("player identity overflow")?;
        self.players
            .try_reserve(added)
            .map_err(|_| "local roster allocation failed")?;
        for id in self.next_id..next {
            self.players.push(LocalPlayer {
                id: PlayerId(id),
                input: None,
            });
        }
        self.next_id = next;
        self.players.truncate(count);
        if count == 1 {
            self.players[0].input = None;
        }
        Ok(())
    }
    pub fn assign(
        &mut self,
        player: PlayerId,
        host: SettingsHost,
        identity: &str,
    ) -> Result<(), String> {
        if !self.needs_assignment() {
            return Err("solo input is automatic; assignment is unnecessary".into());
        }
        if host != self.host
            || identity.is_empty()
            || identity.len() > MAX_VALUE_BYTES
            || identity
                .chars()
                .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
        {
            return Err("invalid native keyboard identity".into());
        }
        let index = self
            .players
            .iter()
            .position(|p| p.id == player)
            .ok_or("local player unavailable")?;
        if self
            .players
            .iter()
            .any(|p| p.id != player && p.input.as_deref() == Some(identity))
        {
            return Err("input device already assigned to another local player".into());
        }
        self.players[index].input = Some(identity.into());
        Ok(())
    }
    pub fn clear(&mut self, player: PlayerId) -> Result<(), String> {
        self.players
            .iter_mut()
            .find(|p| p.id == player)
            .ok_or("local player unavailable")?
            .input = None;
        Ok(())
    }
    pub fn seal(&self) -> Result<InputPlan, String> {
        if !self.needs_assignment() {
            return Ok(InputPlan::Automatic {
                player: self.players[0].id,
            });
        }
        self.players
            .iter()
            .map(|p| {
                p.input
                    .clone()
                    .map(|id| (p.id, id))
                    .ok_or_else(|| format!("player {} needs an input device", p.id.0))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(InputPlan::Assigned)
    }
}
impl InputPlan {
    /// Resolve fresh attachments off-thread; different native aliases may not share a source.
    pub fn resolve(
        &self,
        mut lookup: impl FnMut(&str) -> Result<DeviceId, String>,
    ) -> Result<Vec<(PlayerId, DeviceId)>, String> {
        let Self::Assigned(assignments) = self else {
            return Err("automatic solo input has no explicit attachment routes".into());
        };
        if !(2..=MAX_LOCAL_PLAYERS).contains(&assignments.len()) {
            return Err("invalid assigned player count".into());
        }
        let mut resolved = Vec::with_capacity(assignments.len());
        let mut identities = std::collections::BTreeSet::new();
        for (player, identity) in assignments {
            if identity.is_empty()
                || identity.len() > MAX_VALUE_BYTES
                || identity
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
                || !identities.insert(identity.as_str())
                || resolved.iter().any(|(existing, _)| existing == player)
            {
                return Err("invalid or duplicate local input route".into());
            }
            let source = lookup(identity)?;
            if resolved.iter().any(|(_, existing)| *existing == source) {
                return Err(
                    "multiple native identities resolve to the same physical source".into(),
                );
            }
            resolved.push((*player, source));
        }
        Ok(resolved)
    }
}
#[cfg(test)]
mod fixtures {
    use super::*;
    #[test]
    fn solo_and_four_player_routes_preserve_identity_and_reject_aliases() {
        let mut roster = LocalPlayers::new(SettingsHost::Windows, 8).unwrap();
        assert!(!roster.needs_assignment());
        assert!(matches!(
            roster.seal().unwrap(),
            InputPlan::Automatic { .. }
        ));
        roster.resize(4).unwrap();
        assert!(roster.seal().is_err());
        let ids: Vec<_> = roster.players().iter().map(|p| p.id).collect();
        for (index, id) in ids.iter().enumerate() {
            roster
                .assign(*id, SettingsHost::Windows, &format!("keyboard-{index}"))
                .unwrap();
        }
        let accepted = roster.seal().unwrap();
        assert!(
            roster
                .assign(ids[1], SettingsHost::Windows, "keyboard-0")
                .is_err()
        );
        assert_eq!(roster.seal().unwrap(), accepted);
        assert!(accepted.resolve(|_| Ok(DeviceId(1))).is_err());
        let routes = accepted
            .resolve(|identity| {
                Ok(DeviceId(
                    identity.rsplit('-').next().unwrap().parse::<u64>().unwrap() + 1,
                ))
            })
            .unwrap();
        assert_eq!(routes.len(), 4);
        assert!(roster.resize(9).is_err());
        assert_eq!(roster.players().len(), 4);
        roster.resize(1).unwrap();
        assert_eq!(roster.players()[0].id, ids[0]);
        assert_eq!(roster.players()[0].input(), None);
        roster.resize(3).unwrap();
        assert!(roster.players()[1].id > ids[3]);
        assert!(
            roster
                .assign(ids[3], SettingsHost::Windows, "gone")
                .is_err()
        );
    }
}
