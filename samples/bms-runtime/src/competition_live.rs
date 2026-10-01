//! Shared native application flags and observation outside the audio callback.
use crate::{
    competition::{Competition, OpponentKind},
    local_players::PlayerId,
    multiplayer::{
        Multiplayer, MultiplayerEvent, MultiplayerOptions, Progress, competition_identity,
    },
    player::{self, CompetitionSnapshot, GhostSnapshot, NetworkSnapshot, NetworkStatus},
    replay_capture::LiveReplayCapture,
    replay_playback::read_replay,
};
use beatkernel::{
    input::CodecLimits,
    judge::JudgeEngine,
    replay::codec::ReplayCodecLimits,
    runtime::RuntimeReport,
    time::{ClockDomainId, Timestamp},
};
use beatkernel_bms::{BmsChart, ParseOptions, parse_seeded};
use std::{
    fs::File,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Explicit connection role; only one peer is admitted in the initial mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetworkRole {
    /// Listen on this address without silently selecting public interfaces.
    Host(SocketAddr),
    /// Join the supplied address without DNS or service discovery.
    Join(SocketAddr),
}

/// Competition features are opt-in and retained independently of native options.
#[derive(Clone, Debug)]
pub struct CompetitionOptions {
    /// Saved own/other replays; at most eight opponents.
    pub ghosts: Vec<(OpponentKind, PathBuf)>,
    /// Optional two-peer connection.
    pub network: Option<NetworkRole>,
    /// Finite initial connection/identity exchange deadline.
    pub setup_timeout: Duration,
}
impl Default for CompetitionOptions {
    fn default() -> Self {
        Self {
            ghosts: Vec::new(),
            network: None,
            setup_timeout: Duration::from_secs(10),
        }
    }
}
impl CompetitionOptions {
    /// Extract only application flags; preserve all native flag/value ordering.
    pub fn extract(args: &[String]) -> Result<(Self, Vec<String>)> {
        let mut options = Self::default();
        let mut rest = Vec::new();
        let mut index = 0;
        let mut timeout_seen = false;
        while index < args.len() {
            let flag = args[index].as_str();
            if !matches!(
                flag,
                "--ghost-self" | "--ghost-other" | "--mp-host" | "--mp-join" | "--mp-timeout-ms"
            ) {
                rest.push(args[index].clone());
                // Existing native CLIs use pairs; consume both so a value that
                // happens to look like a competition flag remains a value.
                if flag != "--help" {
                    index += 1;
                    rest.push(
                        args.get(index)
                            .ok_or("native option requires a value")?
                            .clone(),
                    );
                }
                index += 1;
                continue;
            }
            let value = args
                .get(index + 1)
                .filter(|value| !value.is_empty())
                .ok_or("competition option requires a nonempty value")?;
            match flag {
                "--ghost-self" | "--ghost-other" => {
                    if options.ghosts.len() == 8 {
                        return Err("at most eight replay opponents are supported".into());
                    }
                    options.ghosts.push((
                        if flag == "--ghost-self" {
                            OpponentKind::Own
                        } else {
                            OpponentKind::Other
                        },
                        value.into(),
                    ));
                }
                "--mp-host" | "--mp-join" => {
                    if options.network.is_some() {
                        return Err("choose exactly one multiplayer role".into());
                    }
                    let address: SocketAddr = value.parse()?;
                    if address.port() == 0 {
                        return Err("multiplayer requires a nonzero port".into());
                    }
                    if flag == "--mp-join" && address.ip().is_unspecified() {
                        return Err("cannot join an unspecified address".into());
                    }
                    options.network = Some(if flag == "--mp-host" {
                        NetworkRole::Host(address)
                    } else {
                        NetworkRole::Join(address)
                    });
                }
                "--mp-timeout-ms" => {
                    if timeout_seen {
                        return Err("duplicate multiplayer timeout".into());
                    }
                    let millis: u64 = value.parse()?;
                    if !(100..=120_000).contains(&millis) {
                        return Err("multiplayer timeout must be 100..120000ms".into());
                    }
                    options.setup_timeout = Duration::from_millis(millis);
                    timeout_seen = true;
                }
                _ => unreachable!(),
            }
            index += 2;
        }
        if timeout_seen && options.network.is_none() {
            return Err("multiplayer timeout requires host or join".into());
        }
        Ok((options, rest))
    }

    /// Read finite replay files and validate against the actual local setup.
    pub fn load_opponents(
        &self,
        source: &BmsChart,
        competition: &mut Competition,
        limits: ReplayCodecLimits,
    ) -> Result<()> {
        for (kind, path) in &self.ghosts {
            let file = read_replay(&mut File::open(path)?, limits)?;
            competition.add_replay(source, file, limits, *kind, path.display().to_string())?;
        }
        Ok(())
    }
}

/// Current application replay byte/operation caps, without audio preparation.
pub fn replay_limits() -> Result<ReplayCodecLimits> {
    Ok(ReplayCodecLimits::new(
        64 * 1024 * 1024,
        1_000_000,
        4096,
        CodecLimits::new(65536, 32768)?,
    )?)
}

/// Read a bounded UTF-8 or Shift-JIS BMS chart without loading its sound assets.
pub fn load_chart(path: &Path) -> Result<BmsChart> {
    load_chart_with_seed(path, 0)
}

/// Read and resolve BMS conditional branches using a caller-selected chart seed.
/// Replay reconstruction must receive this same resolved source; its current
/// header seed describes judge rules, not BMS source branch provenance.
pub fn load_chart_with_seed(path: &Path, seed: u64) -> Result<BmsChart> {
    let options = ParseOptions::default();
    let text = crate::chart_text::read_chart_text(&mut File::open(path)?, options.max_bytes)?;
    Ok(parse_seeded(&text, options, seed)?)
}

/// Per-play competition state. Socket work never runs on the gameplay thread.
pub struct LiveCompetition {
    player: PlayerId,
    competition: Competition,
    network: Option<Multiplayer>,
    last_publish: Option<i64>,
    last_display: Option<i64>,
    network_failed: bool,
    network_status: Option<NetworkStatus>,
    last_presentation: Option<Instant>,
}
impl LiveCompetition {
    /// Load ghosts before starting audio; spawn networking only when selected.
    pub fn prepare(
        options: &CompetitionOptions,
        source: &BmsChart,
        judge: &JudgeEngine,
        domain: ClockDomainId,
    ) -> Result<Option<Self>> {
        Self::prepare_for(PlayerId(1), options, source, judge, domain)
    }

    /// Prepare comparisons for a stable member of the shared local session.
    pub fn prepare_for(
        player: PlayerId,
        options: &CompetitionOptions,
        source: &BmsChart,
        judge: &JudgeEngine,
        domain: ClockDomainId,
    ) -> Result<Option<Self>> {
        Self::prepare_for_at(player, options, source, judge, domain, Timestamp::ZERO)
    }

    /// Prepare comparisons for a fresh recorded practice section.
    pub fn prepare_at(
        options: &CompetitionOptions,
        source: &BmsChart,
        judge: &JudgeEngine,
        domain: ClockDomainId,
        start: Timestamp,
    ) -> Result<Option<Self>> {
        Self::prepare_for_at(PlayerId(1), options, source, judge, domain, start)
    }

    /// Section identity is shared by local captures, ghosts and network setup.
    pub fn prepare_for_at(
        player: PlayerId,
        options: &CompetitionOptions,
        source: &BmsChart,
        judge: &JudgeEngine,
        domain: ClockDomainId,
        start: Timestamp,
    ) -> Result<Option<Self>> {
        Self::prepare_for_at_with_chart_seed(player, options, source, judge, domain, start, 0)
    }

    /// Prepares solo comparisons with explicit BMS source branch provenance.
    pub fn prepare_at_with_chart_seed(
        options: &CompetitionOptions,
        source: &BmsChart,
        judge: &JudgeEngine,
        domain: ClockDomainId,
        start: Timestamp,
        chart_seed: u64,
    ) -> Result<Option<Self>> {
        Self::prepare_for_at_with_chart_seed(
            PlayerId(1),
            options,
            source,
            judge,
            domain,
            start,
            chart_seed,
        )
    }

    /// Shares the selected branch seed across capture, ghosts and network identity.
    pub fn prepare_for_at_with_chart_seed(
        player: PlayerId,
        options: &CompetitionOptions,
        source: &BmsChart,
        judge: &JudgeEngine,
        domain: ClockDomainId,
        start: Timestamp,
        chart_seed: u64,
    ) -> Result<Option<Self>> {
        if player.0 == 0 {
            return Err("competition player ID must be nonzero".into());
        }
        if options.ghosts.is_empty() && options.network.is_none() {
            return Ok(None);
        }
        let limits = replay_limits()?;
        let capture =
            LiveReplayCapture::new_at_with_chart_seed(judge, domain, limits, start, chart_seed)?;
        let header = capture.header().clone();
        let mut competition = Competition::new(header.clone(), 8)?;
        options.load_opponents(source, &mut competition, limits)?;
        let identity = competition_identity(&header, env!("CARGO_PKG_VERSION"), limits)?;
        let settings = MultiplayerOptions {
            setup_timeout: options.setup_timeout,
            ..MultiplayerOptions::default()
        };
        let network = match options.network {
            Some(NetworkRole::Host(address)) => {
                Some(Multiplayer::host(address, identity, settings)?)
            }
            Some(NetworkRole::Join(address)) => {
                Some(Multiplayer::join(address, identity, settings)?)
            }
            None => None,
        };
        let mut prepared = Self {
            player,
            competition,
            network,
            last_publish: None,
            last_display: None,
            network_failed: false,
            network_status: options.network.as_ref().map(|_| NetworkStatus::Waiting),
            last_presentation: None,
        };
        prepared.publish_presentation(true)?;
        Ok(Some(prepared))
    }

    fn publish_presentation(&mut self, force: bool) -> Result<()> {
        if !player::attached()
            || (!force
                && self
                    .last_presentation
                    .is_some_and(|last| last.elapsed() < Duration::from_millis(50)))
        {
            return Ok(());
        }
        let ghosts = self
            .competition
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
        player::publish_competition(
            self.player,
            CompetitionSnapshot {
                ghosts,
                network: self.network_status.map(|status| NetworkSnapshot {
                    status,
                    progress: self.network.as_ref().and_then(Multiplayer::remote_progress),
                }),
            },
        )?;
        self.last_presentation = Some(Instant::now());
        Ok(())
    }

    /// Observe actual admitted runtime results; remote data never enters judge.
    pub fn observe(&mut self, report: &RuntimeReport) -> Result<()> {
        self.competition
            .observe(&report.judge_events, report.song_time)?;
        let song = report.song_time.as_nanos();
        let mut disconnected = false;
        if let Some(network) = &mut self.network {
            for event in network.poll() {
                match event {
                    MultiplayerEvent::Connected => {
                        self.network_status = Some(NetworkStatus::Connected);
                        println!(
                            "multiplayer peer connected; compatible setup, self-reported progress"
                        );
                    }
                    MultiplayerEvent::Progress(_) => {}
                    MultiplayerEvent::Disconnected(error) => {
                        eprintln!("multiplayer disconnected: {error}; local play continues");
                        disconnected = true;
                    }
                }
            }
            if !self.network_failed
                && !disconnected
                && network.is_connected()
                && self
                    .last_publish
                    .is_none_or(|last| i128::from(song) - i128::from(last) >= 50_000_000)
            {
                let score = self.competition.score();
                match network.try_publish(Progress {
                    song_ns: song,
                    hits: score.hits,
                    misses: score.misses,
                    combo: score.combo,
                    max_combo: score.max_combo,
                }) {
                    Ok(()) => self.last_publish = Some(song),
                    Err(error) => {
                        eprintln!("multiplayer unavailable: {error}; local play continues");
                        disconnected = true;
                    }
                }
            }
        }
        let second = song.div_euclid(1_000_000_000);
        if self.last_display != Some(second) {
            println!(
                "competition local at {song}ns: {:?}",
                self.competition.score()
            );
            for opponent in self.competition.opponents() {
                println!(
                    "competition {:?} ghost={} score={:?} recorded_until={:?}",
                    opponent.kind(),
                    opponent.label(),
                    opponent.score(),
                    opponent.recorded_until()
                );
            }
            if let Some(remote) = self.network.as_ref().and_then(Multiplayer::remote_progress) {
                println!("competition remote={remote:?} (peer reported)");
            }
            self.last_display = Some(second);
        }
        // Retain the owner until native cleanup; joining a socket worker does
        // not belong in the input/advance path, even after a terminal event.
        if disconnected {
            if let Some(network) = &mut self.network {
                network.request_stop();
            }
            self.network_failed = true;
            self.network_status = Some(NetworkStatus::Disconnected);
        }
        self.publish_presentation(disconnected)?;
        Ok(())
    }

    /// Print the exact last local prefix and join networking after native cleanup.
    /// Remote state is the last received prefix, not an authoritative final result.
    pub fn finish(&mut self) {
        println!(
            "competition final local prefix at {:?}: {:?}",
            self.competition.song_time(),
            self.competition.score()
        );
        for opponent in self.competition.opponents() {
            let hit_difference =
                i128::from(self.competition.score().hits) - i128::from(opponent.score().hits);
            println!(
                "competition final {:?} ghost={} score={:?} local_hit_difference={hit_difference:+} recorded_until={:?}",
                opponent.kind(),
                opponent.label(),
                opponent.score(),
                opponent.recorded_until()
            );
        }
        if let Some(network) = &mut self.network {
            for event in network.poll() {
                if let MultiplayerEvent::Disconnected(error) = event {
                    eprintln!("multiplayer final disconnect: {error}");
                    self.network_failed = true;
                }
            }
            if let Some(remote) = network.remote_progress() {
                println!(
                    "competition last peer-reported prefix={remote:?}; independent song time, not a final ranking"
                );
            }
            if let Err(error) = network.stop() {
                eprintln!("multiplayer worker cleanup failed: {error}");
                self.network_failed = true;
            }
            self.network_status = Some(if self.network_failed {
                NetworkStatus::Disconnected
            } else {
                NetworkStatus::Stopped
            });
        }
        if let Err(error) = self.publish_presentation(true) {
            eprintln!("competition presentation cleanup: {error}");
        }
    }
}

fn display_basename(label: &str) -> String {
    // Accept either platform separator without exposing directories in the UI.
    let basename = label.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = basename
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect();
    if clean.is_empty() {
        "RECORD".into()
    } else {
        clean
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    #[test]
    fn display_labels_remove_directories_controls_and_bound_unicode() {
        assert_eq!(display_basename("/private/user/own.bkr"), "own.bkr");
        assert_eq!(display_basename("C:\\private\\other.bkr"), "other.bkr");
        assert_eq!(display_basename("/empty/\n"), "RECORD");
        let label = display_basename(&"🎵".repeat(100));
        assert_eq!(label.chars().count(), 64);
        assert_eq!(label.len(), 256);
    }
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).into()).collect()
    }
    #[test]
    fn extracts_opponents_preserving_native_values() {
        let (options, native) = CompetitionOptions::extract(&args(&[
            "--chart",
            "--ghost-self",
            "--ghost-other",
            "other.bkr",
            "--mp-host",
            "127.0.0.1:1234",
        ]))
        .unwrap();
        assert_eq!(native, args(&["--chart", "--ghost-self"]));
        assert_eq!(options.ghosts.len(), 1);
        assert_eq!(options.ghosts[0].0, OpponentKind::Other);
    }
    #[test]
    fn rejects_conflicting_roles_empty_paths_and_invalid_timeouts() {
        for values in [
            vec!["--ghost-self"],
            vec!["--ghost-self", ""],
            vec!["--mp-host", "127.0.0.1:0"],
            vec!["--mp-timeout-ms", "100"],
            vec!["--mp-host", "127.0.0.1:1234", "--mp-join", "127.0.0.1:1234"],
            vec!["--mp-host", "127.0.0.1:1234", "--mp-timeout-ms", "99"],
        ] {
            assert!(CompetitionOptions::extract(&args(&values)).is_err());
        }
    }
}
