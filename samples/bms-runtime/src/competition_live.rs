//! Shared native application flags and observation outside the audio callback.
use crate::{
    competition::{Competition, OpponentKind},
    competition_progress,
    competition_opponent_loading::{self, OpponentLoadError, OpponentRequest},
    competition_opponent_loader_bridge::NativeOpponentReplayPort,
    competition_terminal::{self, DeliveryStatus, TerminalGuard},
    competition_terminal_bridge::NativeTerminalPort,
    input_sounds::InputSoundIdentity,
    local_players::PlayerId,
    multiplayer::{
        MultiplayerEvent, MultiplayerNotice, MultiplayerOptions, Progress,
        competition_identity_for_section,
    },
    multiplayer_group::{GroupPrefix, MemberProgress},
    multiplayer_quic::QuicCredentials,
    native_competition_network::NativeCompetitionNetwork,
    competition_presentation::{
        self, CompetitionPresentationHost, SoloNetworkPresentation, NetworkStatus,
    },
    competition_presentation_bridge::NativeCompetitionPresentation,
    competition_start_gate::{self, CompetitionSetupControl},
    native_pump_system::SystemControl,
    replay_capture::LiveReplayCapture,
};
use beatkernel::{
    input::CodecLimits,
    judge::JudgeEngine,
    replay::codec::ReplayCodecLimits,
    runtime::RuntimeReport,
    time::{ClockDomainId, Timestamp},
};
use beatkernel_bms::{BmsChart, BmsInputMode, ParseOptions, parse_seeded};
use std::{
    fs::File,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const START_POLICY_FLAGS: [&str; 5] = [
    "--mp-start-lead-ms",
    "--mp-start-min-lead-ms",
    "--mp-clock-max-age-ms",
    "--mp-clock-max-uncertainty-ms",
    "--mp-start-max-lateness-ms",
];

pub use crate::competition_connection::NetworkRole;

/// Competition features are opt-in and retained independently of native options.
#[derive(Clone, Debug)]
pub struct CompetitionOptions {
    /// Explicit native QUIC TLS configuration; incomplete settings stay drafts.
    pub quic: QuicCredentials,
    /// Saved own/other replays; at most eight opponents.
    pub ghosts: Vec<(OpponentKind, PathBuf)>,
    /// Optional bilateral connection or explicitly selected multi-host room.
    pub network: Option<NetworkRole>,
    /// Finite connection/identity and native preparation readiness deadline.
    pub setup_timeout: Duration,
    /// Bounds for software-start negotiation and startup-owner release.
    pub start_policy: crate::multiplayer_start::StartPolicy,
    /// Actual native preroll; supplied by the native preparation entry point.
    pub preroll_ns: i64,
}
impl Default for CompetitionOptions {
    fn default() -> Self {
        Self {
            quic: QuicCredentials::default(),
            ghosts: Vec::new(),
            network: None,
            setup_timeout: Duration::from_secs(10),
            start_policy: crate::multiplayer_start::StartPolicy::default(),
            preroll_ns: 0,
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
        let mut start_seen = [false; 5];
        let mut quic_seen = [false; 4];
        let mut webtransport = [None, None, None];
        let mut room = None;
        while index < args.len() {
            let flag = args[index].as_str();
            if !matches!(
                flag,
                "--ghost-self"
                    | "--ghost-other"
                    | "--mp-host"
                    | "--mp-join"
                    | "--mp-timeout-ms"
                    | "--mp-cert"
                    | "--mp-key"
                    | "--mp-ca"
                    | "--mp-server-name"
                    | "--mp-webtransport"
                    | "--mp-room"
                    | "--mp-role"
                    | "--mp-origin"
            ) && !START_POLICY_FLAGS.contains(&flag)
            {
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
                "--mp-room" => {
                    if room.is_some() {
                        return Err("duplicate room URL".into());
                    }
                    if value.len() > 4096 || value.chars().any(char::is_control) {
                        return Err("invalid bounded room URL".into());
                    }
                    room = Some(value.clone());
                }
                "--mp-webtransport" | "--mp-role" | "--mp-origin" => {
                    let field = match flag {
                        "--mp-webtransport" => 0,
                        "--mp-role" => 1,
                        _ => 2,
                    };
                    if webtransport[field].is_some() {
                        return Err("duplicate WebTransport option".into());
                    }
                    if value.len() > 4096 || value.chars().any(char::is_control) {
                        return Err("invalid bounded WebTransport option".into());
                    }
                    webtransport[field] = Some(value.clone());
                }
                "--mp-cert" | "--mp-key" | "--mp-ca" | "--mp-server-name" => {
                    let field = match flag {
                        "--mp-cert" => 0,
                        "--mp-key" => 1,
                        "--mp-ca" => 2,
                        _ => 3,
                    };
                    if quic_seen[field] {
                        return Err("duplicate QUIC credential option".into());
                    }
                    if value.len() > 4096 || value.chars().any(char::is_control) {
                        return Err("invalid QUIC credential path or server name".into());
                    }
                    match field {
                        0 => options.quic.cert = Some(value.into()),
                        1 => options.quic.key = Some(value.into()),
                        2 => options.quic.ca = Some(value.into()),
                        _ => options.quic.server_name = Some(value.clone()),
                    }
                    quic_seen[field] = true;
                }
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
                _ => {
                    let field = START_POLICY_FLAGS
                        .iter()
                        .position(|candidate| *candidate == flag)
                        .unwrap();
                    if start_seen[field] {
                        return Err("duplicate multiplayer start policy option".into());
                    }
                    let nanos = value
                        .parse::<u64>()?
                        .checked_mul(1_000_000)
                        .ok_or("multiplayer start milliseconds overflow nanoseconds")?;
                    match field {
                        0 => options.start_policy.lead_ns = nanos,
                        1 => options.start_policy.min_remaining_ns = nanos,
                        2 => options.start_policy.max_age_ns = nanos,
                        3 => options.start_policy.max_uncertainty_ns = nanos,
                        _ => options.start_policy.max_release_lateness_ns = nanos,
                    }
                    start_seen[field] = true;
                }
            }
            index += 2;
        }
        if let Some(url) = room {
            if options.network.is_some() || webtransport[0].is_some() || webtransport[1].is_some() {
                return Err("room mode cannot mix bilateral transport or --mp-role".into());
            }
            let origin = webtransport[2]
                .take()
                .ok_or("room mode requires --mp-origin")?;
            if options.quic.ca.is_none() {
                return Err("room mode requires --mp-ca".into());
            }
            options.network = Some(NetworkRole::RoomWebTransport { url, origin });
        } else if webtransport.iter().any(Option::is_some) {
            if options.network.is_some() {
                return Err("WebTransport and raw QUIC modes are mutually exclusive".into());
            }
            let [Some(url), Some(role), Some(origin)] = webtransport else {
                return Err(
                    "WebTransport requires --mp-webtransport, --mp-role and --mp-origin together"
                        .into(),
                );
            };
            let role = match role.as_str() {
                "host" => crate::multiplayer_start::StartRole::Host,
                "join" => crate::multiplayer_start::StartRole::Join,
                _ => return Err("WebTransport --mp-role must be host or join".into()),
            };
            options.network = Some(NetworkRole::WebTransport { url, role, origin });
        }
        if (timeout_seen || start_seen.iter().any(|seen| *seen)) && options.network.is_none() {
            return Err("multiplayer timing options require host or join".into());
        }
        if quic_seen.iter().any(|seen| *seen) {
            match &options.network {
                None => return Err("QUIC credentials require host or join".into()),
                Some(NetworkRole::Host(_)) if quic_seen[2] || quic_seen[3] => {
                    return Err("QUIC host uses certificate/key, not joining trust options".into());
                }
                Some(NetworkRole::Join(_)) if quic_seen[0] || quic_seen[1] => {
                    return Err("QUIC join uses CA/server name, not host credentials".into());
                }
                Some(NetworkRole::WebTransport { .. } | NetworkRole::RoomWebTransport { .. })
                    if quic_seen[0] || quic_seen[1] || quic_seen[3] =>
                {
                    return Err("WebTransport uses CA trust without host certificate/key or a server-name override".into());
                }
                _ => {}
            }
        }
        options.start_policy.validate()?;
        Ok((options, rest))
    }

    /// Read finite replay files and validate against the actual local setup.
    pub fn load_opponents(
        &self,
        source: &BmsChart,
        competition: &mut Competition,
        limits: ReplayCodecLimits,
    ) -> Result<()> {
        if self.ghosts.len() > competition.remaining_opponent_capacity() {
            return Err(crate::competition::CompetitionError::TooManyOpponents.into());
        }
        let mut requests = Vec::new();
        requests.try_reserve_exact(self.ghosts.len())?;
        requests.extend(self.ghosts.iter().map(|(kind, path)| OpponentRequest {
            kind: *kind,
            key: path.as_path(),
        }));
        match competition_opponent_loading::load_opponents(
            &mut NativeOpponentReplayPort,
            source,
            competition,
            &requests,
            limits,
        ) {
            Ok(()) => Ok(()),
            Err(OpponentLoadError::Load(error)) => Err(error),
            Err(OpponentLoadError::Competition(error)) => Err(error.into()),
        }
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
    network: Option<NativeCompetitionNetwork>,
    last_publish: Option<i64>,
    last_display: Option<i64>,
    network_failed: bool,
    network_status: Option<NetworkStatus>,
    last_presentation: Option<u64>,
    network_setup_timeout: Duration,
    terminal: TerminalGuard,
}
impl LiveCompetition {
    pub(crate) fn native_policy_header(&self) -> &beatkernel::replay::ReplayHeader {
        self.competition.expected_header()
    }
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
        Self::prepare_member_section(
            player, options, source, judge, domain, start, chart_seed, None,
        )
    }

    /// Supplies the actual native preroll before files, socket setup or readiness.
    pub fn prepare_native_section_at_with_chart_seed(
        options: &CompetitionOptions,
        source: &BmsChart,
        judge: &JudgeEngine,
        domain: ClockDomainId,
        start: Timestamp,
        chart_seed: u64,
        end: Option<Timestamp>,
        preroll_ns: i64,
    ) -> Result<Option<Self>> {
        if preroll_ns < 0 {
            return Err("native competition preroll cannot be negative".into());
        }
        let mut native_options = options.clone();
        native_options.preroll_ns = preroll_ns;
        Self::prepare_section_at_with_chart_seed(
            &native_options,
            source,
            judge,
            domain,
            start,
            chart_seed,
            end,
        )
    }

    /// Prepare solo competition with an optional original-song endpoint.
    /// Invalid finite geometry rejects before feature selection, files or sockets.
    pub fn prepare_section_at_with_chart_seed(
        options: &CompetitionOptions,
        source: &BmsChart,
        judge: &JudgeEngine,
        domain: ClockDomainId,
        start: Timestamp,
        chart_seed: u64,
        end: Option<Timestamp>,
    ) -> Result<Option<Self>> {
        Self::prepare_member_section(
            PlayerId(1),
            options,
            source,
            judge,
            domain,
            start,
            chart_seed,
            end,
        )
    }

    /// Canonical native saved comparisons for the selected resolved policy.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_native_section_with_policy(
        options: &CompetitionOptions,
        source: &BmsChart,
        judge: &JudgeEngine,
        policy: &crate::play_policy::ResolvedPlayPolicy,
        domain: ClockDomainId,
        start: Timestamp,
        chart_seed: u64,
        end: Option<Timestamp>,
        preroll_ns: i64,
    ) -> Result<Option<Self>> {
        if judge.effective_song_time().is_some() || judge.profile() != policy.judge() {
            return Err("policy-aware competition requires a pristine matching judge".into());
        }
        if policy.selection() == crate::play_policy::GaugeSelection::BeatKernel {
            return Self::prepare_native_section_at_with_chart_seed(
                options, source, judge, domain, start, chart_seed, end, preroll_ns,
            );
        }
        if preroll_ns < 0 {
            return Err("native competition preroll cannot be negative".into());
        }
        let mut native_options = options.clone();
        native_options.preroll_ns = preroll_ns;
        Self::prepare_member_section_with_policy(
            PlayerId(1),
            &native_options,
            source,
            judge,
            policy,
            domain,
            start,
            chart_seed,
            end,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_member_section_with_policy(
        player: PlayerId,
        options: &CompetitionOptions,
        source: &BmsChart,
        judge: &JudgeEngine,
        policy: &crate::play_policy::ResolvedPlayPolicy,
        domain: ClockDomainId,
        start: Timestamp,
        chart_seed: u64,
        end: Option<Timestamp>,
    ) -> Result<Option<Self>> {
        if judge.effective_song_time().is_some() || judge.profile() != policy.judge() {
            return Err("policy-aware competition requires a pristine matching judge".into());
        }
        if policy.selection() == crate::play_policy::GaugeSelection::BeatKernel {
            return Self::prepare_member_section(
                player, options, source, judge, domain, start, chart_seed, end,
            );
        }
        crate::native_judge::validate_policy_competition(policy.selection(), options)?;

        Self::prepare_member_section_inner(
            player,
            options,
            source,
            judge,
            domain,
            start,
            chart_seed,
            end,
            Some(policy.gauge()),
        )
    }
    fn prepare_member_section(
        player: PlayerId,
        options: &CompetitionOptions,
        source: &BmsChart,
        judge: &JudgeEngine,
        domain: ClockDomainId,
        start: Timestamp,
        chart_seed: u64,
        end: Option<Timestamp>,
    ) -> Result<Option<Self>> {
        Self::prepare_member_section_inner(
            player, options, source, judge, domain, start, chart_seed, end, None,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn prepare_member_section_inner(
        player: PlayerId,
        options: &CompetitionOptions,
        source: &BmsChart,
        judge: &JudgeEngine,
        domain: ClockDomainId,
        start: Timestamp,
        chart_seed: u64,
        end: Option<Timestamp>,
        gauge: Option<&crate::gauge::GaugeProfile>,
    ) -> Result<Option<Self>> {
        if options.preroll_ns < 0 {
            return Err("native competition preroll cannot be negative".into());
        }
        if end.is_some_and(|end| start.as_nanos() < 0 || end.as_nanos() < 0 || end <= start) {
            return Err("competition section endpoint must be nonnegative and after start".into());
        }
        if player.0 == 0 {
            return Err("competition player ID must be nonzero".into());
        }
        if options.ghosts.is_empty() && options.network.is_none() {
            return Ok(None);
        }
        let limits = replay_limits()?;
        let input_sounds = InputSoundIdentity::from_source(source)?;
        let capture = match gauge {
            Some(gauge) => LiveReplayCapture::new_with_gauge(
                judge,
                domain,
                limits,
                start,
                chart_seed,
                end,
                BmsInputMode::ButtonOnly,
                input_sounds,
                gauge,
            )?,
            None => LiveReplayCapture::new_with_input_sounds(
                judge,
                domain,
                limits,
                start,
                chart_seed,
                None,
                BmsInputMode::ButtonOnly,
                input_sounds,
            )?,
        };
        let header = capture.header().clone();
        let network_end = if options.network.is_some() { end } else { None };
        let identity = competition_identity_for_section(
            &header,
            env!("CARGO_PKG_VERSION"),
            limits,
            network_end,
        )?;
        let mut competition = Competition::new(header.clone(), 8)?;
        options.load_opponents(source, &mut competition, limits)?;
        let settings = MultiplayerOptions {
            quic: options.quic.clone(),
            setup_timeout: options.setup_timeout,
            start_policy: options.start_policy,
            preroll_ns: options.preroll_ns,
            ..MultiplayerOptions::default()
        };
        let network = match &options.network {
            Some(role) => Some(NativeCompetitionNetwork::new(
                role,
                identity,
                vec![player],
                settings,
            )?),
            None => None,
        };
        Self::from_prepared(player, competition, network, options.setup_timeout).map(Some)
    }

    /// Attach already prepared comparisons and the single selected network
    /// owner; canonical identity preparation precedes this ownership transfer.
    pub(crate) fn from_prepared(
        player: PlayerId,
        competition: Competition,
        network: Option<NativeCompetitionNetwork>,
        setup_timeout: Duration,
    ) -> Result<Self> {
        if player.0 == 0 {
            return Err("competition player ID must be nonzero".into());
        }
        let network_status = network.as_ref().map(|_| NetworkStatus::Waiting);
        let mut prepared = Self {
            player,
            competition,
            network,
            last_publish: None,
            last_display: None,
            network_failed: false,
            network_status,
            last_presentation: None,
            network_setup_timeout: setup_timeout,
            terminal: TerminalGuard::new(),
        };
        prepared.publish_presentation(true)?;
        Ok(prepared)
    }

    /// Read retained comparison evidence after cleanup without native effects.
    pub fn archive_snapshot(&self) -> Result<crate::competition_presentation::CompetitionSnapshot> {
        let network = self
            .network
            .as_ref()
            .filter(|network| !network.is_room())
            .map(|network| SoloNetworkPresentation {
                status: self.network_status,
                roster: network.remote_roster(),
                prefix: network
                    .remote_final_progress()
                    .or_else(|| network.remote_progress()),
            });
        competition_presentation::project_archive_snapshot(self.player, &self.competition, network)
    }

    fn publish_presentation(&mut self, force: bool) -> Result<()> {
        self.publish_presentation_with_host(force, &mut NativeCompetitionPresentation)
    }

    /// Inject display effects without changing observed competition or network policy.
    pub fn publish_presentation_with_host<H: CompetitionPresentationHost>(
        &mut self,
        force: bool,
        host: &mut H,
    ) -> Result<()> {
        let network = self
            .network
            .as_ref()
            .filter(|network| !network.is_room())
            .map(|network| SoloNetworkPresentation {
                status: self.network_status,
                roster: network.remote_roster(),
                prefix: network.remote_progress(),
            });
        competition_presentation::publish_solo(
            host,
            &mut self.last_presentation,
            force,
            self.player,
            &self.competition,
            network,
        )
    }

    /// Observe actual admitted runtime results; remote data never enters judge.
    pub fn observe(&mut self, report: &RuntimeReport) -> Result<()> {
        self.observe_with_presentation(report, &mut NativeCompetitionPresentation)
    }

    pub fn observe_with_presentation<H: CompetitionPresentationHost>(
        &mut self,
        report: &RuntimeReport,
        host: &mut H,
    ) -> Result<()> {
        if self.terminal.is_claimed() {
            return Err("competition already stopped".into());
        }
        self.competition
            .observe(&report.judge_events, report.song_time)?;
        let song = report.song_time.as_nanos();
        let mut disconnected = false;
        if let Some(network) = &mut self.network {
            let mut status = self.network_status.unwrap_or(NetworkStatus::Waiting);
            let active = !self.network_failed;
            if let Err(error) = competition_progress::poll_progress(network, &mut status, active) {
                eprintln!("multiplayer disconnected: {error}; local play continues");
                disconnected = true;
            }
            self.network_status = Some(status);
            let score = self.competition.score();
            let members = [MemberProgress {
                player: self.player,
                progress: Progress {
                    song_ns: song,
                    hits: score.hits,
                    misses: score.misses,
                    combo: score.combo,
                    max_combo: score.max_combo,
                },
            }];
            if network.is_room() {
                competition_progress::observe_room_progress(network, &members)?;
            } else {
                let allowed = !self.network_failed
                    && !disconnected
                    && matches!(status, NetworkStatus::Waiting | NetworkStatus::Connected);
                let due = allowed
                    && self
                        .last_publish
                        .is_none_or(|last| i128::from(song) - i128::from(last) >= 50_000_000);
                match competition_progress::publish_progress(network, &members, allowed, false, due)
                {
                    Ok(true) => self.last_publish = Some(song),
                    Ok(false) => {}
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
            if let Some(remote) = self.network.as_ref().and_then(|network| {
                selected_remote_member(network.remote_roster(), network.remote_progress())
            }) {
                println!(
                    "competition remote player={} progress={:?} (peer reported)",
                    remote.player.0, remote.progress
                );
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
        self.publish_presentation_with_host(disconnected, host)?;
        Ok(())
    }

    /// Native preparation and committed software-start barrier on the game owner.
    /// Service bounded native acquisition and cancellation without judging input.
    pub fn await_network_ready(&mut self, service: impl FnMut() -> Result<bool>) -> Result<bool> {
        self.await_network_start(service, true)
    }
    /// Returns with a future commitment so an already-silent device can arm a frame.
    pub fn await_network_commit(&mut self, service: impl FnMut() -> Result<bool>) -> Result<bool> {
        self.await_network_start(service, false)
    }
    pub fn committed_start_schedule(&self) -> Option<crate::multiplayer_start::StartSchedule> {
        self.network
            .as_ref()
            .and_then(NativeCompetitionNetwork::start_schedule)
    }
    /// Brackets a caller's actual native host read without inventing a clock relation.
    pub fn native_host_bracket(
        &self,
        sample: impl FnOnce() -> Result<beatkernel::time::ClockPoint>,
    ) -> Result<Option<crate::native_start::SessionHostBracket>> {
        let Some(network) = &self.network else {
            return Ok(None);
        };
        let before = network.clock_now_ns()?;
        let host = sample()?;
        let after = network.clock_now_ns()?;
        Ok(Some(crate::native_start::SessionHostBracket::new(
            before, host, after,
        )?))
    }
    pub fn network_clock_now_ns(&self) -> Result<Option<i64>> {
        self.network
            .as_ref()
            .map(NativeCompetitionNetwork::clock_now_ns)
            .transpose()
            .map_err(Into::into)
    }
    fn await_network_start(
        &mut self,
        service: impl FnMut() -> Result<bool>,
        await_release: bool,
    ) -> Result<bool> {
        self.await_network_start_with_ports(
            service,
            await_release,
            &mut SystemControl,
            &mut NativeCompetitionPresentation,
        )
    }

    pub fn await_network_start_with_ports<
        C: CompetitionSetupControl,
        H: CompetitionPresentationHost,
    >(
        &mut self,
        service: impl FnMut() -> Result<bool>,
        await_release: bool,
        control: &mut C,
        presentation: &mut H,
    ) -> Result<bool> {
        if self.terminal.is_claimed() {
            return Err("competition already stopped".into());
        }
        let Some(network) = self.network.as_mut() else {
            return Ok(true);
        };
        let outcome = competition_start_gate::await_start(
            network,
            control,
            self.network_setup_timeout,
            await_release,
            service,
        );
        match &outcome {
            Ok(true) => self.network_status = Some(NetworkStatus::Connected),
            Ok(false) => {
                network.request_stop();
                self.network_status = Some(NetworkStatus::Stopped);
            }
            Err(_) => {
                network.request_stop();
                self.network_failed = true;
                self.network_status = Some(NetworkStatus::Disconnected);
            }
        }
        // Preserve acquisition/protocol errors even if forced display also fails.
        if outcome.is_err() {
            let _ = self.publish_presentation_with_host(true, presentation);
        } else {
            self.publish_presentation_with_host(true, presentation)?;
        }
        outcome
    }

    fn terminal_prefix(&self) -> Option<Progress> {
        let song_ns = self.competition.song_time()?.as_nanos();
        let score = self.competition.score();
        Some(Progress {
            song_ns,
            hits: score.hits,
            misses: score.misses,
            combo: score.combo,
            max_combo: score.max_combo,
        })
    }

    pub(crate) fn mark_native_completed(&mut self) {
        if let Some(network) = &mut self.network {
            network.mark_native_completed();
        }
    }
    /// Actual common native completion proof; independent of cleanup/UI status.
    pub fn native_completed(&self) -> bool {
        self.network
            .as_ref()
            .is_some_and(NativeCompetitionNetwork::native_completed)
    }

    /// Send the exact last observed prefix, wait boundedly for receipt and join.
    /// Call only after native cleanup; a peer receipt is not a ranked final result.
    pub fn finish(&mut self) {
        self.finish_with_presentation(&mut NativeCompetitionPresentation);
    }

    pub fn finish_with_presentation<H: CompetitionPresentationHost>(&mut self, host: &mut H) {
        if !self.terminal.claim() {
            return;
        }
        let member = self.terminal_prefix().map(|progress| MemberProgress {
            player: self.player,
            progress,
        });
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
                if let MultiplayerNotice::Session(MultiplayerEvent::Disconnected(error)) = event {
                    eprintln!("multiplayer final disconnect: {error}");
                    self.network_failed = true;
                }
            }
            let room = network.is_room();
            let intent = competition_terminal::solo_delivery_intent(
                room,
                self.network_failed,
                network.is_ready(),
                member.as_ref(),
            );
            let outcome = competition_terminal::finalize_terminal(
                &mut NativeTerminalPort::new(network),
                intent,
            );
            if let Err(error) = &outcome.delivery {
                eprintln!("multiplayer terminal delivery failed: {error}");
            }
            if let Err(error) = &outcome.cleanup {
                eprintln!("multiplayer worker cleanup failed: {error}");
            }
            if let Err(error) = &outcome.drain {
                eprintln!("multiplayer final disconnect: {error}");
            }
            self.network_failed |= outcome.has_failed();
            if !room && matches!(outcome.delivery, Ok(DeliveryStatus::Accepted)) {
                if let Some(member) = member {
                    println!(
                        "multiplayer terminal prefix acknowledged by peer: {:?}",
                        member.progress
                    );
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            if room {
                if let Some(outcome) = network.room_outcome() {
                    println!(
                        "room cleanup cancelled={} receipts={:?} protocol_error={:?} cleanup_error={:?}",
                        outcome.cancelled, outcome.receipts, outcome.error, outcome.cleanup_error
                    );
                }
            }
            if let Some(remote) =
                selected_remote_member(network.remote_roster(), network.remote_final_progress())
            {
                println!(
                    "competition peer player={} terminal prefix={:?}; self-reported, not a final ranking",
                    remote.player.0, remote.progress
                );
            }
            if let Some(remote) =
                selected_remote_member(network.remote_roster(), network.remote_progress())
            {
                println!(
                    "competition last peer-reported player={} prefix={:?}; independent song time, not a final ranking",
                    remote.player.0, remote.progress
                );
            }
            self.network_status = Some(if self.network_failed {
                NetworkStatus::Disconnected
            } else {
                NetworkStatus::Stopped
            });
        }
        if let Err(error) = self.publish_presentation_with_host(true, host) {
            eprintln!("competition presentation cleanup: {error}");
        }
    }
}

// The accepted roster fixes the sole local member's comparison target. Even a
// valid first row cannot hide an invalid or differently ordered later member.
fn selected_remote_member(
    roster: Option<&[PlayerId]>,
    prefix: Option<&GroupPrefix>,
) -> Option<MemberProgress> {
    competition_presentation::selected_remote_member(roster, prefix)
}

#[cfg(test)]
#[path = "competition_live_group_fixtures.rs"]
mod group_fixtures;

#[cfg(test)]
fn display_basename(label: &str) -> String {
    competition_presentation::display_basename(label)
}

/// Software gate release only; downstream device output latency is separate.
#[cfg(test)]
fn start_release_due(
    schedule: crate::multiplayer_start::StartSchedule,
    now: i64,
    max_lateness_ns: u64,
) -> Result<bool> {
    competition_start_gate::start_release_due(schedule, now, max_lateness_ns)
}

#[cfg(test)]
mod fixtures {
    use super::*;

    #[test]
    fn committed_release_keeps_future_boundary_and_configurable_lateness() {
        let schedule = crate::multiplayer_start::StartSchedule {
            target_ns: 1_000,
            song_target_ns: 1_000,
            uncertainty_ns: 20,
        };
        assert!(!start_release_due(schedule, 999, 25).unwrap());
        assert!(start_release_due(schedule, 1_000, 0).unwrap());
        assert!(start_release_due(schedule, 1_025, 25).unwrap());
        assert!(start_release_due(schedule, 1_026, 25).is_err());
        assert!(start_release_due(schedule, -1, 25).is_err());
        let extreme = crate::multiplayer_start::StartSchedule {
            target_ns: i64::MAX,
            song_target_ns: i64::MAX,
            uncertainty_ns: 0,
        };
        assert!(start_release_due(extreme, i64::MAX, 0).unwrap());
    }

    #[test]
    fn software_start_flags_are_configurable_checked_and_network_only() {
        let values = args(&[
            "--mp-host",
            "127.0.0.1:1234",
            "--mp-start-lead-ms",
            "3000",
            "--mp-start-min-lead-ms",
            "200",
            "--mp-clock-max-age-ms",
            "7000",
            "--mp-clock-max-uncertainty-ms",
            "0",
            "--mp-start-max-lateness-ms",
            "0",
        ]);
        let (options, rest) = CompetitionOptions::extract(&values).unwrap();
        assert!(rest.is_empty());
        assert_eq!(options.start_policy.lead_ns, 3_000_000_000);
        assert_eq!(options.start_policy.min_remaining_ns, 200_000_000);
        assert_eq!(options.start_policy.max_age_ns, 7_000_000_000);
        assert_eq!(options.start_policy.max_uncertainty_ns, 0);
        assert_eq!(options.start_policy.max_release_lateness_ns, 0);
        for invalid in [
            vec!["--mp-start-lead-ms", "3000"],
            vec!["--mp-host", "127.0.0.1:1234", "--mp-start-min-lead-ms", "0"],
            vec!["--mp-host", "127.0.0.1:1234", "--mp-start-lead-ms", "100"],
            vec![
                "--mp-host",
                "127.0.0.1:1234",
                "--mp-start-lead-ms",
                "18446744073709551615",
            ],
            vec![
                "--mp-host",
                "127.0.0.1:1234",
                "--mp-start-lead-ms",
                "3000",
                "--mp-start-lead-ms",
                "4000",
            ],
        ] {
            assert!(CompetitionOptions::extract(&args(&invalid)).is_err());
        }
    }
    #[test]
    fn terminal_prefix_uses_latest_actual_judgments_even_inside_publish_throttle() {
        use beatkernel::{
            audio::command_queue,
            input::BindingMap,
            judge::{JudgeGrade, JudgeProfile, JudgeWindow},
            runtime::Runtime,
            time::{ClockMapper, ClockMappingQuality, ClockPoint},
            transport::{Rate, Transport},
        };
        struct Identity;
        impl ClockMapper for Identity {
            fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
                (from.domain == to).then_some(from.timestamp)
            }
            fn quality(&self) -> ClockMappingQuality {
                ClockMappingQuality::Exact
            }
        }
        let source = parse_seeded(
            "#BPM 60\n#WAV01 key.wav\n#00011:01",
            ParseOptions::default(),
            0,
        )
        .unwrap();
        let judge = JudgeEngine::new(
            source.compile().unwrap().chart,
            source.rules(),
            JudgeProfile::new(
                vec![JudgeWindow {
                    grade: JudgeGrade(1),
                    early: beatkernel::time::Duration::ZERO,
                    late: beatkernel::time::Duration::ZERO,
                }],
                beatkernel::time::Duration::ZERO,
            )
            .unwrap(),
        )
        .unwrap();
        let header = LiveReplayCapture::new(&judge, ClockDomainId(17), replay_limits().unwrap())
            .unwrap()
            .into_file()
            .header;
        let mut owner = LiveCompetition {
            player: PlayerId(1),
            competition: Competition::new(header, 0).unwrap(),
            network: None,
            last_publish: Some(0),
            last_display: None,
            network_failed: false,
            network_status: None,
            last_presentation: None,
            network_setup_timeout: Duration::from_secs(10),
            terminal: TerminalGuard::new(),
        };
        assert_eq!(owner.terminal_prefix(), None); // No invented prefix before an actual report.
        assert!(
            owner
                .await_network_ready(|| panic!("offline competition must not acquire or wait"))
                .unwrap()
        );
        assert!(
            owner
                .await_network_commit(|| panic!("offline commit must not wait"))
                .unwrap()
        );
        assert_eq!(owner.committed_start_schedule(), None);
        assert_eq!(owner.network_clock_now_ns().unwrap(), None);
        assert_eq!(
            owner
                .native_host_bracket(|| panic!("offline owner must not sample a session bridge"))
                .unwrap(),
            None
        );
        assert_eq!(owner.network_status, None);
        let (producer, _consumer) = command_queue(1).unwrap();
        let mut runtime = Runtime::new(
            ClockDomainId(17),
            ClockDomainId(17),
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
            BindingMap::from_bindings([]).unwrap(),
            judge,
            producer,
            vec![],
            0,
        )
        .unwrap();
        let point = ClockPoint {
            domain: ClockDomainId(17),
            timestamp: Timestamp::from_nanos(1),
        };
        let report = runtime.advance_to(point, &Identity, point).unwrap();
        assert_eq!(report.judge_events.len(), 1);
        owner
            .competition
            .observe(&report.judge_events, report.song_time)
            .unwrap();
        assert_eq!(
            owner.terminal_prefix(),
            Some(Progress {
                song_ns: 1,
                hits: 0,
                misses: 1,
                combo: 0,
                max_combo: 0
            })
        );
        assert_eq!(owner.last_publish, Some(0));
        owner.competition.observe(&[], Timestamp::MAX).unwrap();
        assert_eq!(owner.terminal_prefix().unwrap().song_ns, i64::MAX);
        assert_eq!(owner.terminal_prefix().unwrap().misses, 1);
        owner.competition.reset();
        assert_eq!(owner.terminal_prefix(), None);
    }
    #[test]
    fn invalid_finite_geometry_precedes_noop_and_opponent_socket_acquisition() {
        use beatkernel::judge::{JudgeGrade, JudgeProfile, JudgeWindow};
        let source = parse_seeded(
            "#BPM 120\n#WAV01 head.wav\n#00011:01",
            ParseOptions::default(),
            3,
        )
        .unwrap();
        let judge = JudgeEngine::new(
            source.compile().unwrap().chart,
            source.rules(),
            JudgeProfile::new(
                vec![JudgeWindow {
                    grade: JudgeGrade(1),
                    early: beatkernel::time::Duration::ZERO,
                    late: beatkernel::time::Duration::ZERO,
                }],
                beatkernel::time::Duration::ZERO,
            )
            .unwrap(),
        )
        .unwrap();
        let inactive = CompetitionOptions::default();
        let selected = CompetitionOptions {
            ghosts: vec![(
                OpponentKind::Own,
                PathBuf::from("must-not-open-invalid-section.bkr"),
            )],
            network: Some(NetworkRole::Host("127.0.0.1:12345".parse().unwrap())),
            ..CompetitionOptions::default()
        };
        for options in [&inactive, &selected] {
            let error = LiveCompetition::prepare_native_section_at_with_chart_seed(
                options,
                &source,
                &judge,
                ClockDomainId(17),
                Timestamp::ZERO,
                3,
                None,
                -1,
            )
            .err()
            .unwrap();
            assert_eq!(
                error.to_string(),
                "native competition preroll cannot be negative"
            );
            for (start, end) in [(-1, 1), (0, -1), (0, 0), (1, 1), (2, 1)] {
                let error = LiveCompetition::prepare_section_at_with_chart_seed(
                    options,
                    &source,
                    &judge,
                    ClockDomainId(17),
                    Timestamp::from_nanos(start),
                    3,
                    Some(Timestamp::from_nanos(end)),
                )
                .err()
                .unwrap();
                assert_eq!(
                    error.to_string(),
                    "competition section endpoint must be nonnegative and after start"
                );
            }
        }
        assert!(
            LiveCompetition::prepare_section_at_with_chart_seed(
                &inactive,
                &source,
                &judge,
                ClockDomainId(17),
                Timestamp::ZERO,
                3,
                Some(Timestamp::from_nanos(1))
            )
            .unwrap()
            .is_none()
        );
        assert!(
            LiveCompetition::prepare_section_at_with_chart_seed(
                &inactive,
                &source,
                &judge,
                ClockDomainId(17),
                Timestamp::from_nanos(i64::MAX - 1),
                u64::MAX,
                Some(Timestamp::MAX)
            )
            .unwrap()
            .is_none()
        );
        assert!(
            LiveCompetition::prepare_at_with_chart_seed(
                &inactive,
                &source,
                &judge,
                ClockDomainId(17),
                Timestamp::ZERO,
                3
            )
            .unwrap()
            .is_none()
        );
        assert!(
            LiveCompetition::prepare_section_at_with_chart_seed(
                &inactive,
                &source,
                &judge,
                ClockDomainId(17),
                Timestamp::ZERO,
                3,
                None
            )
            .unwrap()
            .is_none()
        );
    }
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

#[cfg(test)]
#[path = "competition_solo_terminal_fixtures.rs"]
mod solo_terminal_fixtures;
