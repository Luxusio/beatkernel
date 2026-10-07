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
    next_id: Option<u32>,
    players: Vec<LocalPlayer>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputPlan {
    Automatic { player: PlayerId },
    Assigned(Vec<(PlayerId, String)>),
}

/// Immutable canonical input ownership after host attachment resolution.
/// Sources are session identities, without native paths or platform tags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedInputPlan {
    members: Vec<(PlayerId, Option<DeviceId>)>,
}

impl ResolvedInputPlan {
    /// One member may use automatic routing; multiple members need distinct exact sources.
    pub fn new(members: Vec<(PlayerId, Option<DeviceId>)>) -> Result<Self, String> {
        validate_source_routes(members.iter().copied())?;
        Ok(Self { members })
    }

    pub fn members(&self) -> &[(PlayerId, Option<DeviceId>)] {
        &self.members
    }

    /// Four u32 words per member: player, selector (0 automatic/1 exact), source low/high.
    /// Automatic rows require zero source words; an exact DeviceId(0) remains valid.
    pub fn from_words(words: &[u32]) -> Result<Self, String> {
        if words.is_empty() || words.len() > MAX_LOCAL_PLAYERS * 4 || words.len() % 4 != 0 {
            return Err("local source plan requires 1..64 complete four-word rows".into());
        }
        let mut members = Vec::new();
        members
            .try_reserve_exact(words.len() / 4)
            .map_err(|_| "local source plan allocation failed")?;
        for row in words.chunks_exact(4) {
            let source = match row[1] {
                0 if row[2] == 0 && row[3] == 0 => None,
                0 => return Err("automatic local source must have zero payload words".into()),
                1 => Some(DeviceId(u64::from(row[2]) | (u64::from(row[3]) << 32))),
                _ => return Err("unknown local source selector".into()),
            };
            members.push((PlayerId(row[0]), source));
        }
        Self::new(members)
    }

    /// Preserve member order and all source bits in the bounded numeric host bridge.
    pub fn to_words(&self) -> Vec<u32> {
        let mut words = Vec::with_capacity(self.members.len() * 4);
        for (player, source) in &self.members {
            let (selector, value) = match source {
                None => (0, 0),
                Some(source) => (1, source.0),
            };
            words.extend_from_slice(&[player.0, selector, value as u32, (value >> 32) as u32]);
        }
        words
    }
}

/// Shared bounded setup validation for host plans and actual Runtime members.
pub(crate) fn validate_source_routes(
    routes: impl IntoIterator<Item = (PlayerId, Option<DeviceId>)>,
) -> Result<(), String> {
    let mut seen = [(PlayerId(0), None); MAX_LOCAL_PLAYERS];
    let mut count = 0;
    let mut automatic = false;
    for (player, source) in routes {
        if count == MAX_LOCAL_PLAYERS {
            return Err("local runtime requires 1..64 members".into());
        }
        if player.0 == 0 {
            return Err("local player identity must be positive".into());
        }
        if seen[..count].iter().any(|(prior, _)| *prior == player) {
            return Err("duplicate local player identity".into());
        }
        if let Some(source) = source {
            if seen[..count]
                .iter()
                .any(|(_, prior)| *prior == Some(source))
            {
                return Err("local input device is assigned more than once".into());
            }
        } else {
            automatic = true;
        }
        seen[count] = (player, source);
        count += 1;
    }
    if count == 0 {
        return Err("local runtime requires 1..64 members".into());
    }
    if count > 1 && automatic {
        return Err("multiple local players require exact devices".into());
    }
    Ok(())
}

impl LocalPlayers {
    pub fn new(host: SettingsHost, capacity: usize) -> Result<Self, String> {
        if !(1..=MAX_LOCAL_PLAYERS).contains(&capacity) {
            return Err("invalid local player capacity".into());
        }
        Ok(Self {
            host,
            capacity,
            next_id: Some(2),
            players: vec![LocalPlayer {
                id: PlayerId(1),
                input: None,
            }],
        })
    }
    /// Restores 2..=64 unique positive player identities and exact assignments.
    /// Exhausted ID space is retained explicitly; existing u32::MAX IDs remain valid.
    pub fn from_assignments(
        host: SettingsHost,
        capacity: usize,
        assignments: Vec<(PlayerId, String)>,
    ) -> Result<Self, String> {
        if !(1..=MAX_LOCAL_PLAYERS).contains(&capacity)
            || !(2..=capacity).contains(&assignments.len())
        {
            return Err("invalid assigned local player count or capacity".into());
        }
        let mut max_id = 0;
        for (index, (player, identity)) in assignments.iter().enumerate() {
            if player.0 == 0
                || !valid_identity(identity)
                || assignments[..index]
                    .iter()
                    .any(|(prior, path)| prior == player || path == identity)
            {
                return Err("invalid or duplicate local player identity/assignment".into());
            }
            max_id = max_id.max(player.0);
        }
        let next_id = max_id.checked_add(1);
        let mut players = Vec::new();
        players
            .try_reserve_exact(assignments.len())
            .map_err(|_| "local roster allocation failed")?;
        players.extend(assignments.into_iter().map(|(id, identity)| LocalPlayer {
            id,
            input: Some(identity),
        }));
        Ok(Self {
            host,
            capacity,
            next_id,
            players,
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
        let allocation = if added == 0 {
            None
        } else {
            let start = self
                .next_id
                .ok_or("local player identity space exhausted")?;
            let last = start
                .checked_add(u32::try_from(added - 1).map_err(|_| "player count overflow")?)
                .ok_or("player identity overflow")?;
            Some((start, last))
        };
        self.players
            .try_reserve(added)
            .map_err(|_| "local roster allocation failed")?;
        if let Some((start, last)) = allocation {
            for id in start..=last {
                self.players.push(LocalPlayer {
                    id: PlayerId(id),
                    input: None,
                });
            }
            self.next_id = last.checked_add(1);
        }
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
        if host != self.host || !valid_identity(identity) {
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
fn valid_identity(identity: &str) -> bool {
    !identity.is_empty()
        && identity.len() <= MAX_VALUE_BYTES
        && !identity
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
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
        // Refuse the complete native draft before invoking any attachment lookup.
        for (index, (player, identity)) in assignments.iter().enumerate() {
            if player.0 == 0
                || !valid_identity(identity)
                || assignments[..index]
                    .iter()
                    .any(|(prior, path)| prior == player || path == identity)
            {
                return Err("invalid or duplicate local input route".into());
            }
        }
        let mut resolved = Vec::new();
        resolved
            .try_reserve_exact(assignments.len())
            .map_err(|_| "local source resolution allocation failed")?;
        for (player, identity) in assignments {
            let source = lookup(identity)?;
            resolved.push((*player, source));
            // Preserve the real lookup prefix: stop at the first source alias.
            validate_source_routes(
                resolved
                    .iter()
                    .map(|(player, source)| (*player, Some(*source))),
            )?;
        }
        Ok(resolved)
    }
}
#[cfg(test)]
mod fixtures {
    use super::*;
    #[test]
    fn restored_assignments_keep_order_and_allocate_after_maximum_retired_id() {
        let mut roster = LocalPlayers::from_assignments(
            SettingsHost::Linux,
            64,
            vec![
                (PlayerId(9), "/dev/input/event0".into()),
                (PlayerId(3), "/dev/input/event1".into()),
            ],
        )
        .unwrap();
        assert_eq!(
            roster
                .players()
                .iter()
                .map(|player| player.id)
                .collect::<Vec<_>>(),
            vec![PlayerId(9), PlayerId(3)]
        );
        roster.resize(1).unwrap();
        roster.resize(4).unwrap();
        assert_eq!(
            roster
                .players()
                .iter()
                .map(|player| player.id)
                .collect::<Vec<_>>(),
            vec![PlayerId(9), PlayerId(10), PlayerId(11), PlayerId(12)]
        );
        let accepted = roster.players().to_vec();
        assert!(roster.resize(65).is_err());
        assert_eq!(roster.players(), accepted);
        for assignments in [
            vec![(PlayerId(0), "a".into()), (PlayerId(2), "b".into())],
            vec![(PlayerId(1), "a".into()), (PlayerId(1), "b".into())],
            vec![(PlayerId(1), "a".into()), (PlayerId(2), "a".into())],
            vec![(PlayerId(1), "".into()), (PlayerId(2), "b".into())],
            vec![(PlayerId(1), "bad\npath".into()), (PlayerId(2), "b".into())],
        ] {
            assert!(LocalPlayers::from_assignments(SettingsHost::Linux, 64, assignments).is_err());
        }
        assert!(
            LocalPlayers::from_assignments(
                SettingsHost::Linux,
                1,
                vec![(PlayerId(1), "a".into()), (PlayerId(2), "b".into())]
            )
            .is_err()
        );
        assert!(
            LocalPlayers::from_assignments(
                SettingsHost::Linux,
                64,
                vec![(PlayerId(1), "a".into())]
            )
            .is_err()
        );
    }
    #[test]
    fn maximum_restored_id_remains_valid_but_exhausted_growth_never_reuses_it() {
        let mut exhausted = LocalPlayers::from_assignments(
            SettingsHost::Linux,
            64,
            vec![(PlayerId(5), "a".into()), (PlayerId(u32::MAX), "b".into())],
        )
        .unwrap();
        assert_eq!(exhausted.next_id, None);
        exhausted.resize(2).unwrap();
        exhausted.resize(1).unwrap();
        let before = exhausted.players().to_vec();
        assert!(exhausted.resize(2).is_err());
        assert_eq!(exhausted.players(), before);
        assert_eq!(exhausted.next_id, None);
        let mut last_slot = LocalPlayers::from_assignments(
            SettingsHost::Linux,
            64,
            vec![
                (PlayerId(5), "a".into()),
                (PlayerId(u32::MAX - 1), "b".into()),
            ],
        )
        .unwrap();
        last_slot.resize(3).unwrap();
        assert_eq!(last_slot.players()[2].id, PlayerId(u32::MAX));
        assert_eq!(last_slot.next_id, None);
        let before = last_slot.players().to_vec();
        assert!(last_slot.resize(4).is_err());
        assert_eq!(last_slot.players(), before);
    }
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
