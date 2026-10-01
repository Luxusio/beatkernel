//! Actual BMS/WAV assets, explicit evdev sources and exact native ALSA output.
#[cfg(any(target_os = "linux", test))]
use beatkernel::audio::AudioCommand;
use beatkernel::{
    audio::{AudioFormat, AudioLimits},
    time::{ClockPair, ClockPoint, Timestamp},
};
use std::{
    collections::{BTreeMap, HashSet},
    error::Error,
    path::PathBuf,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
struct Options {
    chart: PathBuf,
    record_replay: Option<PathBuf>,
    replay_max_records: usize,
    replay_max_bytes: usize,
    evdev: PathBuf,
    local_inputs: Vec<PathBuf>,
    local_players: Vec<beatkernel_bms_runtime::local_players::PlayerId>,
    alsa: String,
    format: AudioFormat,
    period: u32,
    buffer: u32,
    seconds: Option<u64>,
    bindings: BTreeMap<u8, u16>,
    early: i64,
    late: i64,
    offset: i64,
    preroll: i64,
    start_ns: i64,
    bgm_lookahead: i64,
    advance_lag: i64,
    voices: usize,
    mono_stereo: bool,
}
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl Options {
    fn song_origin(&self) -> Result<beatkernel::time::Timestamp> {
        Ok(beatkernel::time::Timestamp::from_nanos(
            self.start_ns
                .checked_sub(self.preroll)
                .ok_or("section start minus preroll overflows song time")?,
        ))
    }
}
fn parse(args: &[String]) -> Result<Options> {
    let (mut chart, mut evdev, mut alsa) = (None, None, None);
    let (mut rate, mut channels, mut period, mut buffer, mut seconds) =
        (None, None, None, None, None);
    let mut local_inputs = Vec::new();
    let mut local_players = Vec::new();
    let mut local_form = None;
    let mut bindings = BTreeMap::new();
    let mut keys = HashSet::new();
    let mut seen = HashSet::new();
    let mut record_replay = None;
    let mut replay_max_records = 1_000_000usize;
    let mut replay_max_bytes = 64 * 1024 * 1024usize;
    let (mut early, mut late, mut offset, mut preroll) =
        (150_000_000i64, 150_000_000i64, 0i64, 3_000_000_000i64);
    let mut advance_lag = 2_000_000i64;
    let mut start_ns = 0i64;
    let mut bgm_lookahead = 3_000_000_000i64;
    let mut voices = 256usize;
    let mut mono_stereo = false;
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let value = args.next().ok_or("every option requires a value")?;
        if !matches!(flag.as_str(), "--bind" | "--local-input" | "--local-player")
            && !seen.insert(flag.as_str())
        {
            return Err(format!("duplicate option {flag}").into());
        }
        match flag.as_str() {
            "--record-replay" => {
                if value.is_empty() {
                    return Err("replay path must be nonempty".into());
                }
                record_replay = Some(PathBuf::from(value));
            }
            "--replay-max-records" => {
                replay_max_records = value.parse()?;
                if replay_max_records == 0 {
                    return Err("replay record limit must be positive usize".into());
                }
            }
            "--replay-max-bytes" => {
                replay_max_bytes = value.parse()?;
                if replay_max_bytes == 0 {
                    return Err("replay byte limit must be positive usize".into());
                }
            }
            "--chart" if !value.is_empty() => chart = Some(PathBuf::from(value)),
            "--evdev" if !value.is_empty() => evdev = Some(PathBuf::from(value)),
            "--local-input" if !value.is_empty() => {
                if local_form == Some(true) {
                    return Err("--local-input and --local-player cannot be mixed".into());
                }
                let path = PathBuf::from(value);
                if local_inputs.len() == beatkernel_bms_runtime::local_players::MAX_LOCAL_PLAYERS
                    || local_inputs.contains(&path)
                {
                    return Err("local input devices must be distinct, at most 64".into());
                }
                local_form = Some(false);
                local_players.push(beatkernel_bms_runtime::local_players::PlayerId(
                    u32::try_from(local_inputs.len() + 1)?,
                ));
                local_inputs.push(path);
            }
            "--local-player" => {
                if local_form == Some(false) {
                    return Err("--local-input and --local-player cannot be mixed".into());
                }
                let (id, path) = value
                    .split_once(':')
                    .ok_or("local player must be ID:PATH")?;
                if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err("local player ID must be positive u32 decimal".into());
                }
                let player = beatkernel_bms_runtime::local_players::PlayerId(id.parse::<u32>()?);
                let path = PathBuf::from(path);
                if player.0 == 0
                    || path.as_os_str().is_empty()
                    || local_inputs.len()
                        == beatkernel_bms_runtime::local_players::MAX_LOCAL_PLAYERS
                    || local_inputs.contains(&path)
                    || local_players.contains(&player)
                {
                    return Err("local players require unique positive u32 IDs and nonempty distinct paths, at most 64".into());
                }
                local_form = Some(true);
                local_players.push(player);
                local_inputs.push(path);
            }
            "--alsa" if !value.is_empty() => alsa = Some(value.clone()),
            "--rate" => rate = Some(value.parse::<u32>()?),
            "--channels" => channels = Some(value.parse::<u16>()?),
            "--period-frames" => period = Some(value.parse::<u32>()?),
            "--buffer-frames" => buffer = Some(value.parse::<u32>()?),
            "--seconds" => {
                let n = value.parse::<u64>()?;
                if !(1..=3600).contains(&n) {
                    return Err("seconds must be 1..3600".into());
                }
                seconds = Some(n);
            }
            "--bind" => {
                let (channel, key) = value
                    .split_once(':')
                    .ok_or("binding must be channelHEX:HIDusageHEX")?;
                let channel = u8::from_str_radix(channel, 16)?;
                let key = u16::from_str_radix(key, 16)?;
                if !matches!(channel, 0x11..=0x19 | 0x21..=0x29) || key == 0 {
                    return Err(
                        "binding needs visible BMS channel and nonzero HID keyboard usage".into(),
                    );
                }
                if bindings.insert(channel, key).is_some() || !keys.insert(key) {
                    return Err("duplicate lane or HID keyboard usage".into());
                }
            }
            "--early-ns" => early = value.parse()?,
            "--late-ns" => late = value.parse()?,
            "--input-offset-ns" => offset = value.parse()?,
            "--bgm-lookahead-ns" => {
                bgm_lookahead = value.parse()?;
                if bgm_lookahead <= 0 {
                    return Err("BGM lookahead must be positive i64 nanoseconds".into());
                }
            }
            "--start-ns" => {
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err("start-ns must be unsigned decimal nanoseconds".into());
                }
                start_ns = value.parse::<i64>()?;
            }
            "--preroll-ns" => preroll = value.parse()?,
            "--advance-lag-ns" => advance_lag = value.parse()?,
            "--voices" => voices = value.parse()?,
            "--channel-policy" => {
                mono_stereo = match value.as_str() {
                    "exact" => false,
                    "mono-stereo" => true,
                    _ => return Err("channel policy must be exact or mono-stereo".into()),
                }
            }
            _ => return Err(format!("unknown or empty option {flag}").into()),
        }
    }
    let period = period.ok_or("explicit --period-frames required")?;
    let buffer = buffer.ok_or("explicit --buffer-frames required")?;
    if period == 0 || period >= buffer || period as usize > AudioLimits::MAX_RENDER_FRAMES {
        return Err("require 0 < period < buffer and period <= 1048576 frames".into());
    }
    if !(0..=1_000_000_000).contains(&advance_lag) {
        return Err("advance lag must be 0..1000000000 ns".into());
    }
    if early < 0
        || late < 0
        || !(0..=10_000_000_000).contains(&preroll)
        || !(1..=AudioLimits::MAX_VOICES).contains(&voices)
    {
        return Err(
            "nonnegative windows, preroll 0..10000000000 ns, and voices 1..4096 required".into(),
        );
    }
    if !local_inputs.is_empty() && (local_inputs.len() < 2 || evdev.is_some()) {
        return Err("use 2..64 --local-input or --local-player assignments, mutually exclusive with --evdev".into());
    }
    let evdev = evdev
        .or_else(|| local_inputs.first().cloned())
        .ok_or("explicit --evdev or multiple local assignments required")?;
    Ok(Options {
        record_replay,
        replay_max_records,
        replay_max_bytes,
        chart: chart.ok_or("explicit --chart required")?,
        evdev,
        local_inputs,
        local_players,
        alsa: alsa.ok_or("explicit --alsa required")?,
        format: AudioFormat::new(
            rate.ok_or("explicit --rate required")?,
            channels.ok_or("explicit --channels required")?,
        )?,
        period,
        buffer,
        seconds,
        bindings,
        early,
        late,
        offset,
        preroll,
        start_ns,
        bgm_lookahead,
        advance_lag,
        voices,
        mono_stereo,
    })
}
#[cfg(test)]
fn shift_bgm(command: AudioCommand, preroll: i64) -> Result<AudioCommand> {
    if !(0..=10_000_000_000).contains(&preroll) {
        return Err("invalid preroll".into());
    }
    let mut feeder = beatkernel_bms_runtime::bgm::BgmFeeder::new(
        vec![command],
        beatkernel_bms_runtime::bgm::BgmConfig {
            output_origin: beatkernel::time::ClockPoint {
                domain: beatkernel::time::ClockDomainId(1),
                timestamp: Timestamp::ZERO,
            },
            sample_rate: 1,
            preroll: beatkernel::time::Duration::from_nanos(preroll),
            lookahead: beatkernel::time::Duration::from_nanos(i64::MAX),
            max_pending: 1,
        },
    )?;
    let mut mapped = None;
    feeder.feed(0, 1, |command| {
        mapped = Some(command);
        Ok(())
    })?;
    mapped.ok_or_else(|| "fixture command beyond feeder horizon".into())
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn estimated_origin(pair: ClockPair, origin: ClockPoint) -> Result<Timestamp> {
    if pair.source.domain != origin.domain {
        return Err("output origin/pair domain mismatch".into());
    }
    let elapsed =
        i128::from(pair.source.timestamp.as_nanos()) - i128::from(origin.timestamp.as_nanos());
    let host = i128::from(pair.target.timestamp.as_nanos()) - elapsed;
    Ok(Timestamp::from_nanos(i64::try_from(host)?))
}
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn watermark(
    origin: ClockPoint,
    last_operation: ClockPoint,
    now: ClockPoint,
    lag: i64,
    backlog: bool,
) -> Result<Option<ClockPoint>> {
    if origin.domain != now.domain
        || last_operation.domain != now.domain
        || !(0..=1_000_000_000).contains(&lag)
    {
        return Err("invalid deadline watermark domain/lag".into());
    }
    if backlog {
        return Ok(None);
    }
    let delayed = i128::from(now.timestamp.as_nanos()) - i128::from(lag);
    let at = delayed
        .max(i128::from(origin.timestamp.as_nanos()))
        .max(i128::from(last_operation.timestamp.as_nanos()));
    Ok(Some(ClockPoint {
        domain: now.domain,
        timestamp: Timestamp::from_nanos(i64::try_from(at)?),
    }))
}
#[cfg(target_os = "linux")]
struct BgmSession(beatkernel_bms_runtime::bgm::BgmFeeder);
#[cfg(target_os = "linux")]
impl std::ops::Deref for BgmSession {
    type Target = beatkernel_bms_runtime::bgm::BgmFeeder;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
#[cfg(target_os = "linux")]
impl std::ops::DerefMut for BgmSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
#[cfg(target_os = "linux")]
impl Drop for BgmSession {
    fn drop(&mut self) {
        println!(
            "BGM feeder config={:?}; final admission summary={:?}; admission does not prove execution/native delivery/acoustic output",
            self.config(),
            self.report()
        );
    }
}
#[cfg(target_os = "linux")]
fn feed_rendered(
    bgm: &mut BgmSession,
    report: Option<beatkernel::audio::RenderReport>,
    admit: impl FnMut(AudioCommand) -> std::result::Result<(), beatkernel::audio::CommandPushError>,
) -> Result<()> {
    if let Some(report) = report {
        let end = report
            .start_frame
            .checked_add(u64::try_from(report.frames)?)
            .ok_or("BGM render cursor overflow")?;
        bgm.feed(end, 256, admit)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
struct DeliverySession(beatkernel::telemetry::InputDeliveryTelemetry);
#[cfg(target_os = "linux")]
impl std::ops::Deref for DeliverySession {
    type Target = beatkernel::telemetry::InputDeliveryTelemetry;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
#[cfg(target_os = "linux")]
impl std::ops::DerefMut for DeliverySession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
#[cfg(target_os = "linux")]
impl Drop for DeliverySession {
    fn drop(&mut self) {
        let observed = self.observed_events();
        match self.summary() {
            Some(summary) => println!(
                "kernel-event-to-runtime delivery age: observed_events={observed}; retained samples={} p50={}ns p95={}ns p99={}ns max={}ns; HOST={:?}, capacity={}; separate from CPU processing; physical input-to-sound unknown",
                summary.samples,
                summary.p50_ns,
                summary.p95_ns,
                summary.p99_ns,
                summary.max_ns,
                self.domain(),
                self.capacity()
            ),
            None => println!(
                "kernel-event-to-runtime delivery age: observed_events={observed}; retained summary unavailable; no zero observation substituted; separate from CPU processing; physical input-to-sound unknown"
            ),
        }
    }
}

#[cfg(target_os = "linux")]
fn save_capture(
    capture: Option<beatkernel_bms_runtime::replay_capture::LiveReplayCapture>,
    path: Option<&std::path::Path>,
    failed_session: bool,
) -> Result<()> {
    let Some(capture) = capture else {
        return Ok(());
    };
    let path = path.ok_or("enabled replay capture missing save path")?;
    let records = capture.records().len();
    let bytes = capture.encoded_bytes();
    println!(
        "replay capture: records={records}, encoded_bytes={bytes}, status={}, path={path:?}; accepted judge operations, physical output unverified",
        if failed_session {
            "valid prefix of failed session"
        } else {
            "complete recorded session"
        }
    );
    let written = capture.save_new(path)?;
    println!("replay create_new saved {written} bytes to {path:?}");
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    run_args(&args)
}

/// Validate settings through the same parsers as play, without opening any resources.
#[allow(dead_code)] // Standalone native binaries have no settings screen.
pub(crate) fn validate_args(args: &[String]) -> Result<()> {
    let (_, native) = beatkernel_bms_runtime::competition_live::CompetitionOptions::extract(args)?;
    parse(&native).map(|_| ())
}

pub(crate) fn run_args(args: &[String]) -> Result<()> {
    let (competition_options, args) =
        beatkernel_bms_runtime::competition_live::CompetitionOptions::extract(args)?;
    if args.is_empty() || args == ["--help"] {
        println!(
            "linux_bms --chart PATH (--evdev NODE | repeated --local-input NODE | repeated --local-player ID:PATH) --alsa ENDPOINT --rate HZ --channels N --period-frames N --buffer-frames N [--seconds N] --bind channelHEX:HIDusageHEX [--bind ...]\nOptions: --record-replay PATH --replay-max-records N --replay-max-bytes N --early-ns N --late-ns N --input-offset-ns N --start-ns N --preroll-ns N --bgm-lookahead-ns N --advance-lag-ns N --voices N --channel-policy exact|mono-stereo\nBounds: start unsigned0..9223372036854775807ns, BGM lookahead positive i64 ns, seconds 1..3600, preroll 0..10000000000 ns, advance lag 0..1000000000 ns, voices 1..4096. Defaults: replay disabled, max records 1000000, max bytes 67108864, BGM lookahead3000000000ns, windows 150000000 ns, offset 0 ns, preroll 3000000000 ns, advance lag 2000000 ns, voices 256, exact channels. Missing --seconds plays the full song through terminal judging and reported native audio presentation; --seconds is a diagnostic loop cutoff after startup. Solo exact one-node bindings; repeated --local-input assigns sequential player IDs to 2..64 devices; repeated --local-player ID:PATH preserves unique positive u32 IDs. Do not mix local forms or --evdev. Exact paths retain colons after the first ID separator. Local cohorts share lane bindings and output. Local replay paths gain .p<ID>.bkr; The winit/wgpu graphical player uses these same native options/local panels; network competition is not yet supported for local groups. Native float32 ALSA, no fallback. Physical timing Unknown."
        );
        return Ok(());
    }
    let options = parse(&args)?;
    #[cfg(target_os = "linux")]
    {
        if options.local_inputs.is_empty() {
            native::run(options, competition_options)
        } else {
            local_native::run(options, competition_options)
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (options, competition_options);
        Err("linux_bms native playback requires Linux".into())
    }
}

#[cfg(target_os = "linux")]
#[path = "linux_bms/local.rs"]
mod local_native;

#[cfg(target_os = "linux")]
mod native {
    use super::*;
    use beatkernel::{
        audio::{Mixer, MixerConfig, PcmLimits, command_queue},
        input::{Binding, BindingMap, DeviceId, DeviceSelector, GameControlId, PhysicalControlId},
        judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
        runtime::RuntimeReport,
        time::{ClockDomainId, ClockMapper, ClockMappingQuality, Duration},
        transport::{Rate, Transport},
    };
    use beatkernel_bms_runtime::local_runtime::SoloRuntime as Runtime;
    use beatkernel_bms_runtime::{ChannelPolicy, load_prepared};
    use beatkernel_platform::{
        audio::{
            DeviceFormat, SampleEncoding,
            presentation::discipline::{
                DisciplineConfig, DisciplineUpdate, PresentationDiscipline,
            },
        },
        linux::{
            AlsaRequest, AlsaStatus, AlsaStream, EvdevDevice, EvdevItem, MonotonicClock,
            alsa_presentation_pair,
        },
    };
    use std::time::{Duration as WallDuration, Instant};
    pub(super) const HOST: ClockDomainId = ClockDomainId(1);
    pub(super) const OUTPUT: ClockDomainId = ClockDomainId(2);
    const DEVICE: DeviceId = DeviceId(1);
    pub(super) struct ExplicitDomains;
    impl ClockMapper for ExplicitDomains {
        fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
            None
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Unknown
        }
    }
    pub(super) fn output_origin() -> ClockPoint {
        ClockPoint {
            domain: OUTPUT,
            timestamp: Timestamp::ZERO,
        }
    }
    pub(super) fn observe(stream: &AlsaStream, startup: bool) -> Result<Option<ClockPair>> {
        match stream.snapshot().status {
            AlsaStatus::Ready if startup => return Ok(None),
            AlsaStatus::Running => {}
            state => return Err(format!("ALSA terminal/unexpected state: {state:?}").into()),
        }
        let Some(timing) = stream.timing_snapshot() else {
            return Ok(None);
        };
        Ok(alsa_presentation_pair(
            timing,
            output_origin(),
            stream.configuration().format.sample_rate(),
        )?)
    }
    pub(super) fn seed(
        stream: &AlsaStream,
        discipline: &mut PresentationDiscipline,
        bgm: &mut BgmSession,
        producer: &mut beatkernel::audio::CommandProducer,
    ) -> Result<ClockPair> {
        let deadline = Instant::now() + WallDuration::from_secs(2);
        while Instant::now() < deadline {
            let pair = observe(stream, true)?;
            feed_rendered(bgm, stream.last_render_report(), |command| {
                producer.try_push(command)
            })?;
            if let Some(pair) = pair {
                discipline.observe_clock_pair(pair)?;
                return Ok(pair);
            }
            std::thread::sleep(WallDuration::from_millis(1));
        }
        Err("no valid native ALSA presentation pair within two seconds".into())
    }
    pub(super) fn schedule(stream: &AlsaStream) -> Result<ClockPoint> {
        let snapshot = stream.snapshot();
        if snapshot.status != AlsaStatus::Running {
            return Err(format!("ALSA terminated: {:?}", snapshot.status).into());
        }
        let rate = u128::from(stream.configuration().format.sample_rate());
        let nanos = (u128::from(snapshot.rendered_frames) * 1_000_000_000).div_ceil(rate);
        Ok(ClockPoint {
            domain: OUTPUT,
            timestamp: Timestamp::from_nanos(i64::try_from(nanos)?),
        })
    }
    fn print_report(
        report: RuntimeReport,
        capture: &mut Option<beatkernel_bms_runtime::replay_capture::LiveReplayCapture>,
        competition: &mut Option<beatkernel_bms_runtime::competition_live::LiveCompetition>,
    ) -> Result<()> {
        if let Some(capture) = capture.as_mut() {
            capture.record_report(&report)?;
        }
        beatkernel_bms_runtime::player::publish_report(&report)?;
        if let Some(competition) = competition.as_mut() {
            competition.observe(&report)?;
        }
        for result in report.judge_events {
            println!("judge={result:?}");
        }
        if !report.audio_failures.is_empty() {
            eprintln!("exact failed audio commands={:?}", report.audio_failures);
        }
        if let Some(error) = report.judge_error {
            return Err(error.into());
        }
        Ok(())
    }
    pub(super) fn run(
        options: Options,
        competition_options: beatkernel_bms_runtime::competition_live::CompetitionOptions,
    ) -> Result<()> {
        let clock = MonotonicClock::new(HOST);
        // Declared before device owners so every exit reports after their cleanup.
        let mut delivery = DeliverySession(beatkernel::telemetry::InputDeliveryTelemetry::new(
            4096, HOST,
        )?);
        let song_origin = options.song_origin()?;
        let prepared = load_prepared(
            &options.chart,
            options.format,
            PcmLimits::new(64 * 1024 * 1024, 256 * 1024 * 1024, 1295)?,
            if options.mono_stereo {
                ChannelPolicy::MonoToStereo
            } else {
                ChannelPolicy::Exact
            },
        )?;
        let (prepared, section) = beatkernel_bms_runtime::section_start::prepare_at(
            prepared,
            Timestamp::from_nanos(options.start_ns),
            PcmLimits::new(64 * 1024 * 1024, 256 * 1024 * 1024, 1295)?,
        )?;
        println!("prepared practice section={section:?}");
        let mut completion = beatkernel_bms_runtime::completion::SongCompletion::prepare(
            &prepared,
            options.late,
            options.offset,
            options.preroll,
            OUTPUT,
        )?;
        for warning in &prepared.source.warnings {
            eprintln!("BMS warning line{}: {}", warning.line, warning.message);
        }
        for note in &prepared.source.notes {
            if !options.bindings.contains_key(&note.lane.channel()) {
                return Err(
                    format!("missing --bind for BMS channel{:02X}", note.lane.channel()).into(),
                );
            }
        }
        let bindings =
            BindingMap::from_bindings(options.bindings.iter().map(|(&channel, &key)| Binding {
                device: DeviceSelector::Exact(DEVICE),
                physical: PhysicalControlId::keyboard(key),
                game_control: GameControlId(u32::from(channel)),
            }))?;
        beatkernel_bms_runtime::player::publish_chart(&prepared.source, &prepared.compiled.chart)?;
        if beatkernel_bms_runtime::player::cancelled() {
            return Ok(());
        }
        let judge = JudgeEngine::new(
            prepared.compiled.chart,
            prepared.source.rules(),
            JudgeProfile::new(
                vec![JudgeWindow {
                    grade: JudgeGrade(1),
                    early: Duration::from_nanos(options.early),
                    late: Duration::from_nanos(options.late),
                }],
                Duration::from_nanos(options.offset),
            )?,
        )?;
        let mut competition =
            beatkernel_bms_runtime::competition_live::LiveCompetition::prepare_at(
                &competition_options,
                &prepared.source,
                &judge,
                HOST,
                Timestamp::from_nanos(options.start_ns),
            )?;
        const SLACK: usize = 1024;
        let capacity = AudioLimits::MAX_COMMANDS;
        let (mut producer, consumer) = command_queue(capacity)?;
        let mut bgm = BgmSession(beatkernel_bms_runtime::bgm::BgmFeeder::new(
            beatkernel_bms_runtime::section_start::relative_commands(
                prepared.bgm_commands,
                Timestamp::from_nanos(options.start_ns),
            )?,
            beatkernel_bms_runtime::bgm::BgmConfig {
                output_origin: beatkernel::time::ClockPoint {
                    domain: OUTPUT,
                    timestamp: Timestamp::ZERO,
                },
                sample_rate: options.format.sample_rate(),
                preroll: Duration::from_nanos(options.preroll),
                lookahead: Duration::from_nanos(options.bgm_lookahead),
                max_pending: capacity - SLACK,
            },
        )?);
        bgm.feed(0, capacity - SLACK, |command| producer.try_push(command))?;
        let mixer = Mixer::new(
            MixerConfig::new(
                options.format,
                OUTPUT,
                Timestamp::ZERO,
                AudioLimits::new(
                    capacity,
                    options.voices,
                    capacity,
                    options.period as usize,
                    capacity,
                )?,
            ),
            prepared.bank,
            consumer,
        )?;
        // Open evdev first: stream RAII/drop and explicit stop join output before
        // this handle can disappear on setup, startup, or gameplay failure.
        let mut input = EvdevDevice::open(&options.evdev, DEVICE, HOST)?;
        let request = AlsaRequest {
            device: options.alsa,
            format: DeviceFormat::new(
                options.format.sample_rate(),
                options.format.channels(),
                SampleEncoding::Float32,
                None,
            )?,
            buffer_frames: options.buffer,
            period_frames: options.period,
            allow_size_rounding: false,
            monotonic_domain: HOST,
        };
        let mut stream = AlsaStream::open(request, mixer)?;
        println!(
            "requested/applied ALSA={:?}; evdev={:?}; exact source={:?}; bindings={:?}; windows={}/{}ns offset={}ns preroll={}ns advance_lag={}ns voices={} channel_policy={} queue/pending={} live_slack={SLACK}",
            stream.configuration(),
            input.descriptor(),
            DEVICE,
            options.bindings,
            options.early,
            options.late,
            options.offset,
            options.preroll,
            options.advance_lag,
            options.voices,
            if options.mono_stereo {
                "mono-stereo"
            } else {
                "exact"
            },
            capacity
        );
        let mut before_origin = 0u64;
        let mut capture = None;
        let outcome = (|| -> Result<()> {
            if options.record_replay.is_some() {
                let limits = beatkernel::replay::codec::ReplayCodecLimits::new(
                    options.replay_max_bytes,
                    options.replay_max_records,
                    4096,
                    beatkernel::input::CodecLimits::new(65536, 32768)?,
                )?;
                capture = Some(
                    beatkernel_bms_runtime::replay_capture::LiveReplayCapture::new_at(
                        &judge,
                        HOST,
                        limits,
                        Timestamp::from_nanos(options.start_ns),
                    )?,
                );
            }
            stream.start()?;
            let mut discipline = PresentationDiscipline::new(
                DisciplineConfig::default(),
                output_origin(),
                HOST,
                song_origin,
            )?;
            let pair = seed(&stream, &mut discipline, &mut bgm, &mut producer)?;
            let host_origin = ClockPoint {
                domain: HOST,
                timestamp: estimated_origin(pair, output_origin())?,
            };
            let transport = Transport::new(host_origin.timestamp, song_origin, Rate::NORMAL);
            println!(
                "estimated output-zero host={host_origin:?}; actual seed={pair:?}; discipline={:?}; quality={:?}; physical latency unmeasured",
                discipline.config(),
                discipline.quality()
            );
            if options.preroll == 0 {
                println!("zero preroll permits startup consumption of initial BGM/notes");
            }
            let mut runtime = Runtime::new(
                HOST,
                OUTPUT,
                transport,
                bindings,
                judge,
                producer,
                prepared.sounds,
                4096,
            )?;
            let deadline = options
                .seconds
                .map(|seconds| Instant::now() + WallDuration::from_secs(seconds));
            let mut last_song = song_origin;
            let mut last_operation = host_origin;
            let mut last_progress = None;
            let pump_outcome = (|| -> Result<()> {
                while deadline.is_none_or(|deadline| Instant::now() < deadline)
                    && !beatkernel_bms_runtime::player::cancelled()
                {
                    if let Some(pair) = observe(&stream, false)? {
                        discipline.observe_clock_pair(pair)?;
                    }
                    feed_rendered(&mut bgm, stream.last_render_report(), |command| {
                        runtime.enqueue_audio(command)
                    })?;
                    discipline.validate_host(clock.now()?)?;
                    let mut backlog = true;
                    for _ in 0..256 {
                        match input.read_next()? {
                            EvdevItem::WouldBlock => {
                                backlog = false;
                                break;
                            }
                            EvdevItem::Ignored => {}
                            EvdevItem::Dropped => {
                                return Err(
                                    "evdev SYN_DROPPED: explicit cleanup/restart required".into()
                                );
                            }
                            EvdevItem::Resync(_) => {
                                return Err(
                                    "evdev Resync barrier: explicit cleanup/restart required"
                                        .into(),
                                );
                            }
                            EvdevItem::Event(event) => {
                                let host = ClockPoint {
                                    domain: event.meta().clock_domain,
                                    timestamp: event.meta().timestamp,
                                };
                                let acquired_now = clock.now()?;
                                discipline.validate_host(acquired_now)?;
                                if host.domain == HOST && host.timestamp > acquired_now.timestamp {
                                    return Err(
                                    "evdev kernel timestamp is ahead of fresh monotonic host time"
                                        .into(),
                                );
                                }
                                if host.domain == HOST && host.timestamp < host_origin.timestamp {
                                    before_origin = before_origin.saturating_add(1);
                                    if before_origin == 1 {
                                        eprintln!(
                                            "ignoring pre-output-origin acquired input (original timestamp retained): {host:?}"
                                        );
                                    }
                                    continue;
                                }
                                discipline.validate_host(host)?;
                                delivery.observe(host, acquired_now)?;
                                print_report(
                                    runtime.process_input(
                                        event,
                                        &ExplicitDomains,
                                        schedule(&stream)?,
                                    )?,
                                    &mut capture,
                                    &mut competition,
                                )?;
                                last_operation = host;
                            }
                        }
                    }
                    let now = clock.now()?;
                    discipline.validate_host(now)?;
                    if let DisciplineUpdate::Applied {
                        base_rate_ppm,
                        correction_ppm,
                        applied_rate_ppm,
                        phase_error_ns,
                        limited,
                    } = discipline.update(now, runtime.transport_mut())?
                    {
                        println!(
                            "discipline measured={base_rate_ppm:+}ppm correction={correction_ppm:+}ppm applied={applied_rate_ppm:+}ppm phase={phase_error_ns}ns limited={limited} quality={:?}",
                            discipline.quality()
                        );
                    }
                    if let Some(at) = watermark(
                        host_origin,
                        last_operation,
                        now,
                        options.advance_lag,
                        backlog,
                    )? {
                        let report =
                            runtime.advance_to(at, &ExplicitDomains, schedule(&stream)?)?;
                        last_operation = at;
                        last_song = report.song_time;
                        let nanos = last_song.as_nanos();
                        let second = nanos.div_euclid(1_000_000_000);
                        if last_progress != Some(second) {
                            if nanos < 0 {
                                println!(
                                    "logical countdown={}s song={nanos}ns",
                                    (-i128::from(nanos) + 999_999_999) / 1_000_000_000
                                );
                            } else {
                                println!("logical song={nanos}ns");
                            }
                            last_progress = Some(second);
                        }
                        print_report(report, &mut capture, &mut competition)?;
                    }
                    if completion.observe(
                        runtime.judge(),
                        last_song,
                        bgm.report(),
                        stream.last_render_report(),
                        discipline.latest_pair().map(|pair| pair.source),
                    )? {
                        println!(
                            "full song completed: terminal judge, drained BGM/mixer and native presentation frontier"
                        );
                        break;
                    }
                    std::thread::sleep(WallDuration::from_millis(1));
                }
                Ok(())
            })();
            println!(
                "runtime processing={:?} counters={:?}; pre-origin ignored={before_origin}; evdev={:?}",
                runtime.telemetry().processing(),
                runtime.telemetry().counters(),
                input.counters()
            );
            pump_outcome
        })();
        let timing = stream.timing_snapshot();
        let stop = stream.stop(); // joins and tears down native handles before evdev drop
        match stream.last_render_report() {
            Some(report) => println!(
                "last successful Mixer render report={report:?}; execution counters distinct from queue admission/native writes; physical delivery unverified"
            ),
            None => println!(
                "last successful Mixer render report unavailable; no render observation substituted"
            ),
        }
        println!(
            "final independent ALSA counters={:?}; last separately coherent timing={timing:?}; evdev={:?}; pre-origin ignored={before_origin}; physical latency=unmeasured",
            stream.snapshot(),
            input.counters()
        );
        if let Err(error) = &stop {
            eprintln!("ALSA stop/join error: {error}");
        }
        // Final input counters were read above; close evdev before file I/O.
        drop(input);
        if let Some(competition) = competition.as_mut() {
            competition.finish();
        }
        let save = save_capture(
            capture,
            options.record_replay.as_deref(),
            outcome.is_err() || stop.is_err(),
        );
        if let Err(error) = &save {
            eprintln!(
                "replay save error after cleanup (valid captured prefix retained until save): {error}"
            );
        }
        outcome?;
        stop?;
        save?;
        Ok(())
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::{
        audio::{SampleId, VoiceId},
        time::ClockDomainId,
    };
    fn point(n: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(1),
            timestamp: Timestamp::from_nanos(n),
        }
    }
    #[test]
    fn practice_start_is_unsigned_bounded_singleton_and_retains_checked_song_origin() {
        let base = args();
        assert_eq!(parse(&base).unwrap().start_ns, 0);
        for start in [0, i64::MAX] {
            let mut configured = base.clone();
            configured.extend(["--start-ns".into(), start.to_string()]);
            let options = parse(&configured).unwrap();
            assert_eq!(options.start_ns, start);
            assert_eq!(
                options.song_origin().unwrap().as_nanos(),
                start.checked_sub(options.preroll).unwrap()
            );
        }
        for value in ["", "-1", "+1", "1.5", "9223372036854775808"] {
            let mut invalid = base.clone();
            invalid.extend(["--start-ns".into(), value.into()]);
            assert!(parse(&invalid).is_err());
        }
        let mut duplicate = base;
        duplicate.extend([
            "--start-ns".into(),
            "1".into(),
            "--start-ns".into(),
            "2".into(),
        ]);
        assert!(parse(&duplicate).is_err());
    }
    #[test]
    fn settings_validation_preserves_native_and_competition_constraints() {
        let mut configured = args();
        configured.extend([
            "--ghost-self".into(),
            "unopened-opponent.bkr".into(),
            "--mp-host".into(),
            "127.0.0.1:34567".into(),
        ]);
        assert!(validate_args(&configured).is_ok());
        for (flag, value) in [
            ("--unknown-setting", "1"),
            ("--backend", "invalid"),
            ("--early-ns", "-1"),
            ("--ghost-other", ""),
            ("--mp-timeout-ms", "99"),
        ] {
            let mut invalid = configured.clone();
            invalid.extend([flag.into(), value.into()]);
            assert!(validate_args(&invalid).is_err());
        }
    }
    #[test]
    fn full_song_is_default_and_explicit_seconds_remains_a_cutoff() {
        let mut supplied = args();
        assert_eq!(parse(&supplied).unwrap().seconds, Some(10));
        let index = supplied.iter().position(|arg| arg == "--seconds").unwrap();
        supplied.drain(index..index + 2);
        assert_eq!(parse(&supplied).unwrap().seconds, None);
        for value in ["0", "3601", "-1"] {
            let mut invalid = supplied.clone();
            invalid.extend(["--seconds".into(), value.into()]);
            assert!(parse(&invalid).is_err());
        }
    }
    #[test]
    fn bgm_lookahead_cli_is_explicit_positive_and_checked() {
        let base = args();
        assert_eq!(parse(&base).unwrap().bgm_lookahead, 3_000_000_000);
        for value in ["1", "9223372036854775807"] {
            let mut supplied = base.clone();
            supplied.extend(["--bgm-lookahead-ns".into(), value.into()]);
            assert_eq!(
                parse(&supplied).unwrap().bgm_lookahead,
                value.parse::<i64>().unwrap()
            );
        }
        for value in ["0", "-1", "9223372036854775808"] {
            let mut supplied = base.clone();
            supplied.extend(["--bgm-lookahead-ns".into(), value.into()]);
            assert!(parse(&supplied).is_err());
        }
        let mut duplicate = base;
        duplicate.extend([
            "--bgm-lookahead-ns".into(),
            "1".into(),
            "--bgm-lookahead-ns".into(),
            "2".into(),
        ]);
        assert!(parse(&duplicate).is_err());
    }
    #[test]
    fn optional_replay_cli_defaults_paths_and_checked_caps() {
        let base = args();
        let defaults = parse(&base).unwrap();
        assert!(defaults.record_replay.is_none());
        assert_eq!(defaults.replay_max_records, 1_000_000);
        assert_eq!(defaults.replay_max_bytes, 64 * 1024 * 1024);
        let mut enabled = base.clone();
        enabled.extend([
            "--record-replay".into(),
            "capture.bkr".into(),
            "--replay-max-records".into(),
            "1".into(),
            "--replay-max-bytes".into(),
            "4096".into(),
        ]);
        let parsed = parse(&enabled).unwrap();
        assert_eq!(parsed.record_replay, Some(PathBuf::from("capture.bkr")));
        assert_eq!(parsed.replay_max_records, 1);
        assert_eq!(parsed.replay_max_bytes, 4096);
        for flag in ["--replay-max-records", "--replay-max-bytes"] {
            for value in ["0", "-1", "184467440737095516160"] {
                let mut invalid = base.clone();
                invalid.extend([flag.into(), value.into()]);
                assert!(parse(&invalid).is_err());
            }
        }
        let mut empty = base.clone();
        empty.extend(["--record-replay".into(), String::new()]);
        assert!(parse(&empty).is_err());
        for flag in [
            "--record-replay",
            "--replay-max-records",
            "--replay-max-bytes",
        ] {
            let mut duplicate = base.clone();
            duplicate.extend([flag.into(), "1".into(), flag.into(), "2".into()]);
            assert!(parse(&duplicate).is_err());
        }
    }
    fn args() -> Vec<String> {
        [
            "--chart",
            "fixture.bms",
            "--evdev",
            "/dev/input/event4",
            "--alsa",
            "hw:1,0",
            "--rate",
            "48000",
            "--channels",
            "2",
            "--period-frames",
            "256",
            "--buffer-frames",
            "1024",
            "--seconds",
            "10",
            "--bind",
            "11:04",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }
    #[test]
    fn portable_cli_defaults_required_sizes_duplicates_and_bounds() {
        let parsed = parse(&args()).unwrap();
        assert_eq!(parsed.preroll, 3_000_000_000);
        assert_eq!(parsed.advance_lag, 2_000_000);
        assert_eq!(parsed.offset, 0);
        assert_eq!(parsed.voices, 256);
        for (flag, value) in [
            ("--preroll-ns", "-1"),
            ("--preroll-ns", "10000000001"),
            ("--advance-lag-ns", "-1"),
            ("--advance-lag-ns", "1000000001"),
            ("--voices", "4097"),
            ("--bind", "12:04"),
            ("--bind", "11:05"),
        ] {
            let mut supplied = args();
            supplied.extend([flag.into(), value.into()]);
            assert!(parse(&supplied).is_err());
        }
        let mut supplied = args();
        supplied.extend(["--advance-lag-ns".into(), "0".into()]);
        assert_eq!(parse(&supplied).unwrap().advance_lag, 0);
        let mut missing = args();
        missing.drain(8..10);
        assert!(parse(&missing).is_err()); // missing rate
    }
    #[test]
    fn local_input_roster_accepts_four_and_rejects_ambiguous_or_excess_assignments() {
        let mut supplied = args();
        supplied.drain(2..4); // Replace solo assignment with local collection.
        for index in 0..4 {
            supplied.extend(["--local-input".into(), format!("/dev/input/event{index}")]);
        }
        assert_eq!(parse(&supplied).unwrap().local_inputs.len(), 4);
        assert_eq!(
            parse(&supplied).unwrap().local_players,
            (1..=4)
                .map(beatkernel_bms_runtime::local_players::PlayerId)
                .collect::<Vec<_>>()
        );
        let mut duplicate = supplied.clone();
        duplicate.extend(["--local-input".into(), "/dev/input/event0".into()]);
        assert!(parse(&duplicate).is_err());
        let mut mixed = supplied.clone();
        mixed.extend(["--evdev".into(), "/dev/input/event9".into()]);
        assert!(parse(&mixed).is_err());
        let mut single = args();
        single[2] = "--local-input".into();
        assert!(parse(&single).is_err());
        let mut maximum = args();
        maximum.drain(2..4);
        for index in 0..64 {
            maximum.extend(["--local-input".into(), format!("/dev/input/event{index}")]);
        }
        assert_eq!(parse(&maximum).unwrap().local_inputs.len(), 64);
        maximum.extend(["--local-input".into(), "/dev/input/event64".into()]);
        assert!(parse(&maximum).is_err());
    }
    #[test]
    fn tagged_local_players_preserve_sparse_ids_colon_paths_and_strict_roster_rules() {
        use beatkernel_bms_runtime::local_players::PlayerId;
        let mut base = args();
        base.drain(2..4);
        let mut configured = base.clone();
        configured.extend([
            "--local-player".into(),
            "7:/dev/input/by-id/keyboard:a".into(),
            "--local-player".into(),
            "1000:/dev/input/event9".into(),
            "--local-player".into(),
            "4294967295:/dev/input/event10".into(),
        ]);
        let parsed = parse(&configured).unwrap();
        assert_eq!(
            parsed.local_players,
            vec![PlayerId(7), PlayerId(1000), PlayerId(u32::MAX)]
        );
        assert_eq!(
            parsed.local_inputs[0],
            PathBuf::from("/dev/input/by-id/keyboard:a")
        );
        for (flag, value) in [
            ("--local-player", "0:/dev/input/event11"),
            ("--local-player", "7:/dev/input/event11"),
            ("--local-player", "8:/dev/input/event9"),
            ("--local-player", "8:"),
            ("--local-player", "4294967296:/dev/input/event11"),
            ("--local-input", "/dev/input/event11"),
            ("--evdev", "/dev/input/event11"),
        ] {
            let mut invalid = configured.clone();
            invalid.extend([flag.into(), value.into()]);
            assert!(parse(&invalid).is_err());
        }
        let mut reverse_mixed = base.clone();
        reverse_mixed.extend([
            "--local-input".into(),
            "/dev/input/event0".into(),
            "--local-player".into(),
            "7:/dev/input/event1".into(),
        ]);
        assert!(parse(&reverse_mixed).is_err());
        let mut maximum = base;
        for index in 0..64 {
            maximum.extend([
                "--local-player".into(),
                format!("{}:/dev/input/event{index}", index + 1000),
            ]);
        }
        assert_eq!(parse(&maximum).unwrap().local_players.len(), 64);
        maximum.extend(["--local-player".into(), "9999:/dev/input/event64".into()]);
        assert!(parse(&maximum).is_err());
    }
    #[test]
    fn checked_bgm_preroll_preserves_identity_and_rejects_overflow() {
        let command = AudioCommand::Play {
            voice: VoiceId(40000),
            sample: SampleId(10),
            at: Timestamp::from_nanos(12),
            gain: 0.5,
        };
        assert_eq!(shift_bgm(command, 0).unwrap(), command);
        assert!(
            matches!(shift_bgm(command,3_000_000_000).unwrap(),AudioCommand::Play{at,..} if at.as_nanos()==3_000_000_012)
        );
        assert!(
            shift_bgm(
                AudioCommand::Play {
                    voice: VoiceId(1),
                    sample: SampleId(1),
                    at: Timestamp::from_nanos(i64::MAX),
                    gain: 1.0
                },
                1
            )
            .is_err()
        );
    }
    #[test]
    fn finite_lag_deadline_watermark_and_backlog_keep_original_event_floor() {
        assert_eq!(
            watermark(point(10), point(100), point(200), 50, false).unwrap(),
            Some(point(150))
        );
        assert_eq!(
            watermark(point(10), point(100), point(200), 150, false).unwrap(),
            Some(point(100))
        );
        assert_eq!(
            watermark(point(10), point(10), point(11), 100, false).unwrap(),
            Some(point(10))
        );
        assert_eq!(
            watermark(point(10), point(100), point(200), 50, true).unwrap(),
            None
        );
        assert_eq!(
            watermark(
                point(i64::MIN),
                point(i64::MIN),
                point(i64::MIN),
                1_000_000_000,
                false
            )
            .unwrap(),
            Some(point(i64::MIN))
        );
        assert_eq!(
            watermark(point(0), point(i64::MAX), point(i64::MAX), 0, false).unwrap(),
            Some(point(i64::MAX))
        );
        assert!(
            watermark(
                point(0),
                point(0),
                ClockPoint {
                    domain: ClockDomainId(2),
                    timestamp: Timestamp::ZERO
                },
                0,
                false
            )
            .is_err()
        );
    }
    #[test]
    fn estimated_origin_uses_supplied_pair_and_checks_domain_and_range() {
        let pair = ClockPair {
            source: point(100),
            target: ClockPoint {
                domain: ClockDomainId(2),
                timestamp: Timestamp::from_nanos(1000),
            },
        };
        assert_eq!(
            estimated_origin(pair, point(0)).unwrap(),
            Timestamp::from_nanos(900)
        );
        assert!(
            estimated_origin(
                pair,
                ClockPoint {
                    domain: ClockDomainId(3),
                    timestamp: Timestamp::ZERO
                }
            )
            .is_err()
        );
        assert!(
            estimated_origin(
                ClockPair {
                    source: point(i64::MAX),
                    target: ClockPoint {
                        domain: ClockDomainId(2),
                        timestamp: Timestamp::from_nanos(i64::MIN)
                    }
                },
                point(0)
            )
            .is_err()
        );
    }
}
