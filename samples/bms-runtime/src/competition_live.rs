//! Shared native application flags and observation outside the audio callback.
use crate::{
    competition::{Competition, OpponentKind},
    multiplayer::{
        competition_identity, Multiplayer, MultiplayerEvent, MultiplayerOptions, Progress,
    },
    replay_capture::LiveReplayCapture,
    replay_playback::read_replay,
};
use beatkernel::{
    input::CodecLimits, judge::JudgeEngine, replay::codec::ReplayCodecLimits,
    runtime::RuntimeReport, time::ClockDomainId,
};
use beatkernel_bms::{parse, BmsChart, ParseOptions};
use std::{
    fs::File,
    io::Read,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
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

/// Read a bounded UTF-8 BMS chart without loading its sound assets.
pub fn load_chart(path: &Path) -> Result<BmsChart> {
    let options = ParseOptions::default();
    let mut bytes = Vec::new();
    File::open(path)?
        .take(
            u64::try_from(options.max_bytes)?
                .checked_add(1)
                .ok_or("chart extent overflow")?,
        )
        .read_to_end(&mut bytes)?;
    if bytes.len() > options.max_bytes {
        return Err("BMS text exceeds parser byte cap".into());
    }
    Ok(parse(std::str::from_utf8(&bytes)?, options)?)
}

/// Per-play competition state. Socket work never runs on the gameplay thread.
pub struct LiveCompetition {
    competition: Competition,
    network: Option<Multiplayer>,
    last_publish: Option<i64>,
    last_display: Option<i64>,
    network_failed: bool,
}
impl LiveCompetition {
    /// Load ghosts before starting audio; spawn networking only when selected.
    pub fn prepare(
        options: &CompetitionOptions,
        source: &BmsChart,
        judge: &JudgeEngine,
        domain: ClockDomainId,
    ) -> Result<Option<Self>> {
        if options.ghosts.is_empty() && options.network.is_none() {
            return Ok(None);
        }
        let limits = replay_limits()?;
        let capture = LiveReplayCapture::new(judge, domain, limits)?;
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
        Ok(Some(Self {
            competition,
            network,
            last_publish: None,
            last_display: None,
            network_failed: false,
        }))
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
                    MultiplayerEvent::Connected => println!(
                        "multiplayer peer connected; compatible setup, self-reported progress"
                    ),
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
        }
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
            println!("competition final {:?} ghost={} score={:?} local_hit_difference={hit_difference:+} recorded_until={:?}", opponent.kind(), opponent.label(), opponent.score(), opponent.recorded_until());
        }
        if let Some(network) = &mut self.network {
            for event in network.poll() {
                if let MultiplayerEvent::Disconnected(error) = event {
                    eprintln!("multiplayer final disconnect: {error}");
                }
            }
            if let Some(remote) = network.remote_progress() {
                println!("competition last peer-reported prefix={remote:?}; independent song time, not a final ranking");
            }
            if let Err(error) = network.stop() {
                eprintln!("multiplayer worker cleanup failed: {error}");
            }
        }
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
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
