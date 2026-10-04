//! One native network owner for actual ordered local-member progress.
//! Remote prefixes are display data and never enter any local judge.

use crate::{
    competition_live::{CompetitionOptions, replay_limits},
    input_sounds::InputSoundIdentity,
    local_players::PlayerId,
    local_runtime::MemberConfig,
    multiplayer::{
        MultiplayerError, MultiplayerEvent, MultiplayerNotice, MultiplayerOptions,
        competition_identity_for_section,
    },
    multiplayer_group::{GroupPrefix, MemberProgress, validate_members, validate_roster},
    native_competition_network::NativeCompetitionNetwork,
    native_start::{NativeStartAgreement, NativeStartResult, SessionHostBracket},
    player::{self, NetworkSnapshot, NetworkStatus},
    replay_capture::LiveReplayCapture,
};
use beatkernel::time::{ClockDomainId, ClockPoint, Timestamp};
use beatkernel_bms::{BmsChart, BmsInputMode};
use std::time::{Duration, Instant};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Shared readiness, committed start, progress and cleanup for one whole cohort.
pub struct NativeGroupCompetition {
    network: NativeCompetitionNetwork,
    players: Vec<PlayerId>,
    local: Option<Vec<MemberProgress>>,
    last_publish: Option<Instant>,
    last_presentation: Option<Instant>,
    status: NetworkStatus,
    failure: Option<MultiplayerError>,
    setup_timeout: Duration,
    finished: bool,
}

impl NativeGroupCompetition {
    /// Validate every actual member before acquiring one endpoint for the cohort.
    pub fn prepare(
        options: &CompetitionOptions,
        source: &BmsChart,
        members: &[MemberConfig],
        domain: ClockDomainId,
        start: Timestamp,
        chart_seed: u64,
        end: Option<Timestamp>,
        preroll: i64,
    ) -> Result<Option<Self>> {
        let players = member_roster(members, start, end, preroll)?;
        let Some(role) = &options.network else {
            return Ok(None);
        };
        let identity = canonical_identity(
            options, source, members, domain, start, chart_seed, end, preroll,
        )?;
        let settings = MultiplayerOptions {
            quic: options.quic.clone(),
            setup_timeout: options.setup_timeout,
            start_policy: options.start_policy,
            preroll_ns: preroll,
            ..MultiplayerOptions::default()
        };
        let mut network_players = Vec::new();
        network_players.try_reserve_exact(players.len())?;
        network_players.extend_from_slice(&players);
        let network = NativeCompetitionNetwork::new(role, identity, network_players, settings)?;
        let mut owner = Self {
            network,
            players,
            local: None,
            last_publish: None,
            last_presentation: None,
            status: NetworkStatus::Waiting,
            failure: None,
            setup_timeout: options.setup_timeout,
            finished: false,
        };
        owner.publish_presentation(true)?;
        Ok(Some(owner))
    }

    pub fn is_failed(&self) -> bool {
        self.status == NetworkStatus::Disconnected || self.network.room_failed()
    }

    pub(crate) fn mark_native_completed(&mut self) {
        self.network.mark_native_completed();
    }
    /// Actual common native completion proof; independent of cleanup/UI status.
    pub fn native_completed(&self) -> bool {
        self.network.native_completed()
    }

    /// Retain the whole real prefix before optional network publication. A
    /// networking failure disables comparison without rejecting local progress.
    pub fn observe(&mut self, members: &[MemberProgress]) -> Result<()> {
        if self.finished {
            return Err("group competition already stopped".into());
        }
        if self.network.is_room() {
            // The room controller retains every actual prefix and applies its
            // own network-clock cadence and display-only failure policy.
            self.network.observe_room(members)?;
            return Ok(());
        }
        let next = validated_local_prefix(&self.players, self.local.as_deref(), members)?;
        self.local = Some(next);
        let before = self.status;
        if let Err(error) = self.poll_network() {
            self.disconnect(error);
        }
        if self.failure.is_none()
            && self.network.is_ready()
            && self.network.start_schedule().is_some()
            && self
                .last_publish
                .is_none_or(|last| last.elapsed() >= Duration::from_millis(50))
        {
            let publication = copy_members(members)?;
            match self.network.try_publish(publication) {
                Ok(()) => self.last_publish = Some(Instant::now()),
                Err(error) => self.disconnect(error),
            }
        }
        self.publish_presentation(before != self.status)
    }

    fn poll_network(&mut self) -> std::result::Result<(), MultiplayerError> {
        let mut failure = None;
        let active = !self.finished
            && matches!(
                self.status,
                NetworkStatus::Waiting | NetworkStatus::Connected
            );
        for notice in self.network.poll() {
            match notice {
                MultiplayerNotice::Session(MultiplayerEvent::Connected) if active => {
                    self.status = NetworkStatus::Waiting
                }
                MultiplayerNotice::Session(MultiplayerEvent::Ready) if active => {
                    self.status = NetworkStatus::Connected
                }
                MultiplayerNotice::Session(MultiplayerEvent::Disconnected(error)) => {
                    if failure.is_none() {
                        failure = Some(error);
                    }
                }
                _ => {}
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn disconnect(&mut self, error: MultiplayerError) {
        if self.failure.is_none() {
            eprintln!("group multiplayer unavailable: {error}; local play continues");
            self.failure = Some(error);
        }
        self.status = NetworkStatus::Disconnected;
        self.network.request_stop();
    }

    fn publish_presentation(&mut self, force: bool) -> Result<()> {
        if self.network.is_room()
            || !player::attached()
            || (!force
                && self
                    .last_presentation
                    .is_some_and(|last| last.elapsed() < Duration::from_millis(50)))
        {
            return Ok(());
        }
        let selected = remote_members(
            &self.players,
            self.network.remote_roster(),
            self.network.remote_progress(),
        )?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(selected.len())?;
        for (player, remote) in selected {
            rows.push((
                player,
                NetworkSnapshot {
                    status: self.status,
                    progress: remote.map(|member| member.progress),
                },
            ));
        }
        player::publish_networks(&rows)?;
        self.last_presentation = Some(Instant::now());
        Ok(())
    }

    /// Cleanup only: after an observation, send one actual terminal prefix and
    /// await its application ACK. Always join, including unplayed cancellation.
    pub fn finish(&mut self, members: &[MemberProgress]) -> Result<()> {
        if self.network.is_room() {
            if self.finished {
                return Err("group competition already stopped".into());
            }
            // Actual native completion proof is held by this shared backend;
            // cleanup success and initialized state cannot create it.
            let delivery = copy_members(members)
                .and_then(|members| self.network.finish_delivery(members).map_err(Into::into));
            let joined = self
                .network
                .stop()
                .map_err(Box::<dyn std::error::Error>::from);
            self.finished = true;
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(outcome) = self.network.room_outcome() {
                println!(
                    "room cleanup cancelled={} receipts={:?} protocol_error={:?} cleanup_error={:?}",
                    outcome.cancelled, outcome.receipts, outcome.error, outcome.cleanup_error
                );
            }
            return delivery.and(joined);
        }
        let was_failed = self.is_failed();
        let delivery = (|| -> Result<()> {
            if self.finished {
                return Err("group competition already stopped".into());
            }
            // Preparation/cancellation alone has not observed gameplay. Caller
            // initialization values must not become a fabricated final prefix.
            if self.local.is_none() {
                return Ok(());
            }
            let terminal = validated_local_prefix(&self.players, self.local.as_deref(), members)?;
            let retained = copy_members(&terminal)?;
            self.local = Some(retained);
            if let Err(error) = self.poll_network() {
                self.disconnect(error);
            }
            if let Some(error) = &self.failure {
                return Err(error.clone().into());
            }
            if let Err(error) = self.network.finish_delivery(terminal) {
                self.disconnect(error.clone());
                return Err(error.into());
            }
            Ok(())
        })();
        let joined = self.network.stop().map_err(|error| {
            self.disconnect(error.clone());
            Box::<dyn std::error::Error>::from(error)
        });
        self.finished = true;
        // Joining fences the worker. Drain its remaining accepted notices before
        // publishing the retained final display; EOF alone cannot undo an ACK.
        let drained = match self.poll_network() {
            Ok(()) | Err(MultiplayerError::Closed) => Ok(()),
            Err(error) => {
                self.disconnect(error.clone());
                Err(Box::<dyn std::error::Error>::from(error))
            }
        };
        let reported = (|| -> Result<()> {
            for (player, remote) in remote_members(
                &self.players,
                self.network.remote_roster(),
                self.network.remote_final_progress(),
            )? {
                if let Some(remote) = remote {
                    println!(
                        "competition player={} peer player={} terminal prefix={:?}; self-reported, not a final ranking",
                        player.0, remote.player.0, remote.progress
                    );
                }
            }
            Ok(())
        })();
        self.status = if was_failed
            || delivery.is_err()
            || joined.is_err()
            || drained.is_err()
            || reported.is_err()
        {
            NetworkStatus::Disconnected
        } else {
            NetworkStatus::Stopped
        };
        let presentation = self.publish_presentation(true);
        delivery
            .and(joined)
            .and(drained)
            .and(reported)
            .and(presentation)
    }
}

impl NativeStartAgreement for NativeGroupCompetition {
    fn await_commit(
        &mut self,
        service: &mut dyn FnMut() -> NativeStartResult<bool>,
    ) -> NativeStartResult<bool> {
        let deadline = Instant::now() + self.setup_timeout;
        let outcome = (|| -> Result<bool> {
            if self.finished {
                return Err("group competition already stopped".into());
            }
            if let Some(error) = &self.failure {
                return Err(error.clone().into());
            }
            self.network.try_ready()?;
            loop {
                if !service()? {
                    return Ok(false);
                }
                self.poll_network()?;
                if Instant::now() >= deadline {
                    return Err(MultiplayerError::SetupTimeout.into());
                }
                if self.network.start_schedule().is_some() {
                    return Ok(true);
                }
                std::thread::sleep(
                    Duration::from_millis(5)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
            }
        })();
        match &outcome {
            Ok(true) => self.status = NetworkStatus::Connected,
            Ok(false) => {
                self.network.request_stop();
                self.status = NetworkStatus::Stopped;
            }
            Err(_) => {
                self.network.request_stop();
                self.status = NetworkStatus::Disconnected;
            }
        }
        if outcome.is_err() {
            let _ = self.publish_presentation(true);
        } else {
            self.publish_presentation(true)?;
        }
        outcome
    }
    fn committed_schedule(&self) -> NativeStartResult<crate::multiplayer_start::StartSchedule> {
        self.network
            .start_schedule()
            .ok_or_else(|| "committed start missing".into())
    }
    fn host_bracket(
        &self,
        sample: &mut dyn FnMut() -> NativeStartResult<ClockPoint>,
    ) -> NativeStartResult<SessionHostBracket> {
        let before = self.network.clock_now_ns()?;
        let host = sample()?;
        let after = self.network.clock_now_ns()?;
        Ok(SessionHostBracket::new(before, host, after)?)
    }
    fn clock_now_ns(&self) -> NativeStartResult<i64> {
        Ok(self.network.clock_now_ns()?)
    }
}

fn member_roster(
    members: &[MemberConfig],
    start: Timestamp,
    end: Option<Timestamp>,
    preroll: i64,
) -> Result<Vec<PlayerId>> {
    if start.as_nanos() < 0
        || !(0..=10_000_000_000).contains(&preroll)
        || end.is_some_and(|end| end <= start)
    {
        return Err("invalid native group section/preroll".into());
    }
    if members.is_empty() || members.len() > 64 {
        return Err("native group requires 1..64 members".into());
    }
    let mut players = Vec::new();
    players.try_reserve_exact(members.len())?;
    players.extend(members.iter().map(|member| member.player));
    validate_roster(&players)?;
    Ok(players)
}

pub(crate) fn canonical_identity(
    options: &CompetitionOptions,
    source: &BmsChart,
    members: &[MemberConfig],
    domain: ClockDomainId,
    start: Timestamp,
    chart_seed: u64,
    end: Option<Timestamp>,
    preroll: i64,
) -> Result<Vec<u8>> {
    member_roster(members, start, end, preroll)?;
    options.start_policy.validate()?;
    let limits = replay_limits()?;
    let input_sounds = InputSoundIdentity::from_source(source)?;
    let mut identity = None;
    for member in members {
        let capture = LiveReplayCapture::new_with_input_sounds(
            &member.judge,
            domain,
            limits,
            start,
            chart_seed,
            None,
            BmsInputMode::ButtonOnly,
            input_sounds,
        )?;
        let current = competition_identity_for_section(
            capture.header(),
            env!("CARGO_PKG_VERSION"),
            limits,
            end,
        )?;
        if let Some(expected) = &identity {
            if *expected != current {
                return Err("native group members have different competition identities".into());
            }
        } else {
            crate::replay_playback::validate_setup(source, &capture.into_file(), limits)?;
            identity = Some(current);
        }
    }
    identity.ok_or_else(|| "native group has no identity".into())
}

fn copy_members(members: &[MemberProgress]) -> Result<Vec<MemberProgress>> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(members.len())?;
    copy.extend_from_slice(members);
    Ok(copy)
}

fn validated_local_prefix(
    players: &[PlayerId],
    previous: Option<&[MemberProgress]>,
    members: &[MemberProgress],
) -> Result<Vec<MemberProgress>> {
    validate_roster(players)?;
    validate_members(previous, members)?;
    if players.len() != members.len()
        || players
            .iter()
            .zip(members)
            .any(|(player, member)| *player != member.player)
    {
        return Err("native group progress changed the local roster".into());
    }
    copy_members(members)
}

fn remote_members(
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
#[path = "native_group_competition_fixtures.rs"]
mod fixtures;
