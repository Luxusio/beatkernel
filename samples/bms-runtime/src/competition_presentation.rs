//! Display-only competition projection, with explicit publication effects and time.
use crate::{
    competition::{Competition, OpponentKind},
    local_players::PlayerId,
    multiplayer_group::{GroupPrefix, MemberProgress, validate_members, validate_roster},
};
use beatkernel::time::Timestamp;

pub type PresentationResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type Result<T> = PresentationResult<T>;

/// Actual recorded-operation prefix, with a bounded display basename.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GhostSnapshot {
    pub kind: OpponentKind,
    pub label: String,
    pub hits: u64,
    pub misses: u64,
    pub combo: u64,
    pub max_combo: u64,
    pub recorded_until: Option<Timestamp>,
}
/// Connection lifecycle; peer scores remain explicitly self-reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkStatus {
    Waiting,
    Connected,
    Disconnected,
    Stopped,
}
/// Last peer prefix is retained even after disconnect or cleanup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkSnapshot {
    pub status: NetworkStatus,
    pub progress: Option<crate::multiplayer::Progress>,
}
/// One local player's bounded comparison state; no judge or clock authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompetitionSnapshot {
    pub ghosts: Vec<GhostSnapshot>,
    pub network: Option<NetworkSnapshot>,
}

/// Publication time controls display cadence only; it is never gameplay evidence.
pub trait CompetitionPresentationHost {
    fn attached(&self) -> bool;
    fn now_ns(&mut self) -> PresentationResult<u64>;
    fn publish_saved(
        &mut self,
        player: PlayerId,
        ghosts: Vec<GhostSnapshot>,
    ) -> PresentationResult<()>;
    fn publish_solo(
        &mut self,
        player: PlayerId,
        snapshot: CompetitionSnapshot,
    ) -> PresentationResult<()>;
    fn publish_group(&mut self, rows: &[(PlayerId, NetworkSnapshot)]) -> PresentationResult<()>;
}

/// Suppression precedes projection; refusal never advances the committed cadence.
pub fn publication_due<H: CompetitionPresentationHost>(
    host: &mut H,
    last: Option<u64>,
    force: bool,
) -> PresentationResult<Option<u64>> {
    if !host.attached() {
        return Ok(None);
    }
    let now = host.now_ns()?;
    if let Some(last) = last {
        let elapsed = now
            .checked_sub(last)
            .ok_or("competition presentation clock regressed")?;
        if !force && elapsed < 50_000_000 {
            return Ok(None);
        }
    }
    Ok(Some(now))
}

/// Completion starts the next display interval. A failed post-effect clock
/// leaves cadence unchanged, although the successful publication cannot roll back.
fn commit_publication<H: CompetitionPresentationHost>(
    host: &mut H,
    last: &mut Option<u64>,
    started: u64,
) -> PresentationResult<()> {
    let completed = host.now_ns()?;
    completed
        .checked_sub(started)
        .ok_or("competition presentation clock regressed")?;
    *last = Some(completed);
    Ok(())
}

pub struct SoloNetworkPresentation<'a> {
    pub status: Option<NetworkStatus>,
    pub roster: Option<&'a [PlayerId]>,
    pub prefix: Option<&'a GroupPrefix>,
}

/// Cold archive projection of retained evidence, without UI or cadence effects.
pub fn project_archive_snapshot(
    player: PlayerId,
    competition: &Competition,
    network: Option<SoloNetworkPresentation<'_>>,
) -> Result<CompetitionSnapshot> {
    validate_roster(&[player])?;
    if competition.opponents().len() > 8 {
        return Err("archived opponent count exceeds bound".into());
    }
    let mut ghosts = Vec::new();
    ghosts.try_reserve_exact(competition.opponents().len())?;
    for opponent in competition.opponents() {
        let label = try_display_basename(opponent.label())?;
        let score = opponent.score();
        ghosts.push(GhostSnapshot {
            kind: opponent.kind(),
            label,
            hits: score.hits,
            misses: score.misses,
            combo: score.combo,
            max_combo: score.max_combo,
            recorded_until: opponent.recorded_until(),
        });
    }
    let network = if let Some(network) = network {
        let mapped = remote_members(&[player], network.roster, network.prefix)?;
        network.status.map(|status| NetworkSnapshot {
            status,
            progress: mapped[0].1.map(|member| member.progress),
        })
    } else {
        None
    };
    Ok(CompetitionSnapshot { ghosts, network })
}
/// Room rosters have no admitted one-to-one peer mapping in this policy.
pub fn project_archive_network(
    room: bool,
    local: &[PlayerId],
    status: NetworkStatus,
    remote: Option<&[PlayerId]>,
    prefix: Option<&GroupPrefix>,
) -> Result<Vec<(PlayerId, NetworkSnapshot)>> {
    validate_roster(local)?;
    if room {
        return Ok(Vec::new());
    }
    let mapped = remote_members(local, remote, prefix)?;
    let mut rows = Vec::new();
    rows.try_reserve_exact(mapped.len())?;
    for (player, remote) in mapped {
        rows.push((
            player,
            NetworkSnapshot {
                status,
                progress: remote.map(|member| member.progress),
            },
        ));
    }
    Ok(rows)
}

pub fn publish_solo<H: CompetitionPresentationHost>(
    host: &mut H,
    last: &mut Option<u64>,
    force: bool,
    player: PlayerId,
    competition: &Competition,
    network: Option<SoloNetworkPresentation<'_>>,
) -> PresentationResult<()> {
    let Some(now) = publication_due(host, *last, force)? else {
        return Ok(());
    };
    let ghosts = competition
        .opponents()
        .iter()
        .map(|opponent| {
            let score = opponent.score();
            GhostSnapshot {
                kind: opponent.kind(),
                label: display_basename(opponent.label()),
                hits: score.hits,
                misses: score.misses,
                combo: score.combo,
                max_combo: score.max_combo,
                recorded_until: opponent.recorded_until(),
            }
        })
        .collect();
    if let Some(network) = network {
        let snapshot = CompetitionSnapshot {
            ghosts,
            network: network.status.map(|status| NetworkSnapshot {
                status,
                progress: selected_remote_member(network.roster, network.prefix)
                    .map(|member| member.progress),
            }),
        };
        host.publish_solo(player, snapshot)?;
    } else {
        host.publish_saved(player, ghosts)?;
    }
    commit_publication(host, last, now)
}

pub fn publish_group<H: CompetitionPresentationHost>(
    host: &mut H,
    last: &mut Option<u64>,
    force: bool,
    room: bool,
    local: &[PlayerId],
    status: NetworkStatus,
    remote: Option<&[PlayerId]>,
    prefix: Option<&GroupPrefix>,
) -> PresentationResult<()> {
    if room {
        return Ok(());
    }
    let Some(now) = publication_due(host, *last, force)? else {
        return Ok(());
    };
    let selected = remote_members(local, remote, prefix)?;
    let mut rows = Vec::new();
    rows.try_reserve_exact(selected.len())?;
    for (player, remote) in selected {
        rows.push((
            player,
            NetworkSnapshot {
                status,
                progress: remote.map(|member| member.progress),
            },
        ));
    }
    host.publish_group(&rows)?;
    commit_publication(host, last, now)
}

pub fn selected_remote_member(
    roster: Option<&[PlayerId]>,
    prefix: Option<&GroupPrefix>,
) -> Option<MemberProgress> {
    let roster = roster?;
    validate_roster(roster).ok()?;
    let prefix = prefix?;
    validate_members(None, &prefix.members).ok()?;
    if roster.len() != prefix.members.len()
        || roster
            .iter()
            .zip(&prefix.members)
            .any(|(player, member)| *player != member.player)
    {
        return None;
    }
    prefix.members.first().copied()
}

pub fn display_basename(label: &str) -> String {
    try_display_basename(label).expect("display basename allocation failed")
}
fn try_display_basename(label: &str) -> Result<String> {
    // Accept either platform separator without exposing directories in the UI.
    let basename = label.rsplit(['/', '\\']).next().unwrap_or("");
    let mut clean = String::new();
    clean.try_reserve_exact(basename.len().min(256).max(6))?;
    for character in basename
        .chars()
        .filter(|character| !character.is_control())
        .take(64)
    {
        clean.push(character);
    }
    if clean.is_empty() {
        clean.push_str("RECORD");
    }
    Ok(clean)
}

pub fn remote_members(
    local: &[PlayerId],
    remote: Option<&[PlayerId]>,
    prefix: Option<&GroupPrefix>,
) -> Result<Vec<(PlayerId, Option<MemberProgress>)>> {
    validate_roster(local)?;
    if let Some(remote) = remote {
        validate_roster(remote)?;
    }
    if let Some(prefix) = prefix {
        let remote = remote.ok_or("remote group progress has no accepted roster")?;
        validate_members(None, &prefix.members)?;
        if remote.len() != prefix.members.len()
            || remote
                .iter()
                .zip(&prefix.members)
                .any(|(player, member)| *player != member.player)
        {
            return Err("remote group progress changed the accepted roster".into());
        }
    }
    let mut mapped = Vec::new();
    mapped.try_reserve_exact(local.len())?;
    for (index, player) in local.iter().enumerate() {
        mapped.push((
            *player,
            prefix.and_then(|prefix| prefix.members.get(index)).copied(),
        ));
    }
    Ok(mapped)
}

#[cfg(test)]
#[path = "competition_archive_projection_fixtures.rs"]
mod competition_archive_projection_fixtures;
