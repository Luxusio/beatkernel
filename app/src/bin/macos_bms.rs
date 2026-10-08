//! Actual BMS/WAV assets, exact IORegistry input assignments and shared CoreAudio output.
#[cfg(test)]
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
#[cfg(any(target_os = "macos", test))]
#[path = "macos_bms/input.rs"]
mod collected_input;
type Result<T> = std::result::Result<T, Box<dyn Error>>;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
struct Options {
    chart: PathBuf,
    record_replay: Option<PathBuf>,
    replay_max_records: usize,
    replay_max_bytes: usize,
    device: u32,
    keyboard_registry: u64,
    local_players: Vec<(beatkernel_bms_runtime::local_players::PlayerId, u64)>,
    format: AudioFormat,
    buffer: u32,
    seconds: Option<u64>,
    bindings: BTreeMap<u8, u16>,
    early: i64,
    late: i64,
    offset: i64,
    preroll: i64,
    chart_seed: u64,
    gauge: beatkernel_bms_runtime::play_policy::GaugeSelection,
    start_ns: i64,
    end_ns: Option<i64>,
    bgm_lookahead: i64,
    advance_lag: i64,
    voices: usize,
    mono_stereo: bool,
}
fn local_assignment(value: &str) -> Result<(beatkernel_bms_runtime::local_players::PlayerId, u64)> {
    let (id, registry) = value
        .split_once(':')
        .ok_or("local-player requires ID:REGISTRY")?;
    if id.is_empty()
        || registry.is_empty()
        || !id.bytes().all(|b| b.is_ascii_digit())
        || !registry.bytes().all(|b| b.is_ascii_digit())
    {
        return Err("local player and registry IDs require positive ASCII decimal integers".into());
    }
    let id = id.parse::<u32>()?;
    let registry = registry.parse::<u64>()?;
    if id == 0 || registry == 0 {
        return Err("local player and registry IDs must be positive".into());
    }
    Ok((
        beatkernel_bms_runtime::local_players::PlayerId(id),
        registry,
    ))
}
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl Options {
    fn playback_end(&self) -> Result<Option<u64>> {
        use beatkernel_bms_runtime::{practice::PracticeStart, practice_loop::PracticeLoop};
        self.end_ns
            .map(|end| {
                let start = PracticeStart::from_nanoseconds(self.start_ns)?;
                Ok(
                    PracticeLoop::new(start, PracticeStart::from_nanoseconds(end)?)?
                        .playback_end_frame(
                            start,
                            beatkernel::time::Duration::from_nanos(self.preroll),
                            self.format.sample_rate(),
                        )?,
                )
            })
            .transpose()
    }
    fn song_origin(&self) -> Result<beatkernel::time::Timestamp> {
        Ok(beatkernel::time::Timestamp::from_nanos(
            self.start_ns
                .checked_sub(self.preroll)
                .ok_or("section start minus preroll overflows song time")?,
        ))
    }
}
fn parse(args: &[String]) -> Result<Options> {
    let (mut chart, mut device, mut keyboard_registry) = (None, None, None);
    let (mut rate, mut channels, mut buffer, mut seconds) = (None, None, None, None);
    let mut local_players = Vec::new();
    let mut bindings = BTreeMap::new();
    let mut keys = HashSet::new();
    let mut seen = HashSet::new();
    let mut record_replay = None;
    let mut replay_max_records = 1_000_000usize;
    let mut replay_max_bytes = 64 * 1024 * 1024usize;
    let (mut early, mut late, mut offset, mut preroll) =
        (150_000_000i64, 150_000_000i64, 0i64, 3_000_000_000i64);
    let mut advance_lag = 2_000_000i64;
    let mut chart_seed = 0u64;
    let mut gauge = beatkernel_bms_runtime::play_policy::GaugeSelection::BeatKernel;
    let mut start_ns = 0i64;
    let mut end_ns = None;
    let mut bgm_lookahead = 3_000_000_000i64;
    let mut voices = 256usize;
    let mut mono_stereo = false;
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let value = args.next().ok_or("every option requires a value")?;
        if !matches!(flag.as_str(), "--bind" | "--local-player") && !seen.insert(flag.as_str()) {
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
            "--device" => device = Some(value.parse::<u32>()?),
            "--local-player" => {
                let assignment = local_assignment(value)?;
                if local_players.len() >= beatkernel_bms_runtime::local_players::MAX_LOCAL_PLAYERS
                    || local_players
                        .iter()
                        .any(|&(id, registry)| id == assignment.0 || registry == assignment.1)
                {
                    return Err(
                        "local players require at most64 unique player and registry IDs".into(),
                    );
                }
                local_players.push(assignment);
            }
            "--keyboard-registry" => keyboard_registry = Some(value.parse::<u64>()?),
            "--rate" => rate = Some(value.parse::<u32>()?),
            "--channels" => channels = Some(value.parse::<u16>()?),
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
            "--gauge" => gauge = value.parse()?,
            "--chart-seed" => {
                chart_seed = beatkernel_bms_runtime::settings::parse_chart_seed(value)?;
            }
            "--start-ns" => {
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err("start-ns must be unsigned decimal nanoseconds".into());
                }
                start_ns = value.parse::<i64>()?;
            }
            "--end-ns" => {
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err("end-ns must be unsigned decimal nanoseconds".into());
                }
                end_ns = Some(value.parse::<i64>()?);
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
    let buffer = buffer.ok_or("explicit --buffer-frames required")?;
    let device = device.ok_or("explicit --device required")?;
    let keyboard_registry = if local_players.is_empty() {
        keyboard_registry.ok_or("explicit --keyboard-registry required")?
    } else {
        if local_players.len() < 2 || keyboard_registry.is_some() {
            return Err(
                "local play requires 2..64 assignments and no solo keyboard override".into(),
            );
        }
        0
    };
    if device == 0
        || (local_players.is_empty() && keyboard_registry == 0)
        || buffer == 0
        || buffer as usize > AudioLimits::MAX_RENDER_FRAMES
    {
        return Err("positive device/registry IDs and buffer 1..1048576 frames required".into());
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
    if end_ns.is_some_and(|end| end <= start_ns) {
        return Err("end-ns must be strictly after start-ns".into());
    }
    Ok(Options {
        record_replay,
        replay_max_records,
        replay_max_bytes,
        chart: chart.ok_or("explicit --chart required")?,
        device,
        keyboard_registry,
        local_players,
        format: AudioFormat::new(
            rate.ok_or("explicit --rate required")?,
            channels.ok_or("explicit --channels required")?,
        )?,
        buffer,
        seconds,
        bindings,
        early,
        late,
        offset,
        preroll,
        chart_seed,
        gauge,
        start_ns,
        end_ns,
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

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[cfg(test)]
fn estimated_origin(pair: ClockPair, origin: ClockPoint) -> Result<Timestamp> {
    if pair.source.domain != origin.domain {
        return Err("output origin/pair domain mismatch".into());
    }
    let elapsed =
        i128::from(pair.source.timestamp.as_nanos()) - i128::from(origin.timestamp.as_nanos());
    let host = i128::from(pair.target.timestamp.as_nanos()) - elapsed;
    Ok(Timestamp::from_nanos(i64::try_from(host)?))
}
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[cfg(test)]
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
    if backlog || now.timestamp < origin.timestamp {
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
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[cfg(test)]
fn input_in_epoch(input: ClockPoint, now: ClockPoint, origin: ClockPoint) -> Result<bool> {
    if input.domain != origin.domain || now.domain != origin.domain {
        return Err(
            "IOHID canonical sample/current time differs from normalized HOST domain".into(),
        );
    }
    if input.timestamp > now.timestamp {
        return Err("IOHID acquisition timestamp is ahead of fresh mach host sample".into());
    }
    Ok(input.timestamp >= origin.timestamp)
}
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(test)]
enum PauseInputStage {
    Live,
    Paused,
    AfterResume,
}
/// Original IOHID timestamps choose their pause side; post-resume events are
/// parked until the collector is empty so release reconciliation comes first.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[cfg(test)]
fn pause_input_stage(
    host: ClockPoint,
    paused: Option<ClockPoint>,
    resuming: Option<ClockPoint>,
) -> Result<PauseInputStage> {
    if let Some(at) = resuming {
        if at.domain != host.domain {
            return Err("CoreAudio resume boundary/input clock domain differs".into());
        }
        return Ok(if host.timestamp < at.timestamp {
            PauseInputStage::Paused
        } else {
            PauseInputStage::AfterResume
        });
    }
    if let Some(at) = paused {
        if at.domain != host.domain {
            return Err("CoreAudio pause boundary/input clock domain differs".into());
        }
        if host.timestamp >= at.timestamp {
            return Ok(PauseInputStage::Paused);
        }
    }
    Ok(PauseInputStage::Live)
}
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[cfg(test)]
fn park_resume_event(
    events: &mut Vec<beatkernel::input::PhysicalInputEvent>,
    event: beatkernel::input::PhysicalInputEvent,
) -> Result<()> {
    if events.len() >= 4096 {
        return Err("CoreAudio resume input backlog exceeds4096 events; restart required".into());
    }
    events.push(event);
    Ok(())
}
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[cfg(test)]
fn validate_input_chronology(input: ClockPoint, last_operation: ClockPoint) -> Result<()> {
    if input.domain != last_operation.domain || input.timestamp < last_operation.timestamp {
        return Err("IOHID input host chronology regressed behind the last accepted operation; explicit restart required".into());
    }
    Ok(())
}
#[cfg(target_os = "macos")]
struct BgmSession(beatkernel_bms_runtime::bgm::BgmFeeder);
#[cfg(target_os = "macos")]
impl std::ops::Deref for BgmSession {
    type Target = beatkernel_bms_runtime::bgm::BgmFeeder;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
#[cfg(target_os = "macos")]
impl std::ops::DerefMut for BgmSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
#[cfg(target_os = "macos")]
impl Drop for BgmSession {
    fn drop(&mut self) {
        println!(
            "BGM feeder config={:?}; final admission summary={:?}; admission does not prove execution/native delivery/acoustic output",
            self.config(),
            self.report()
        );
    }
}
#[cfg(target_os = "macos")]
use beatkernel_bms_runtime::native_audio::feed_rendered;

#[cfg(target_os = "macos")]
struct DeliverySession(beatkernel::telemetry::InputDeliveryTelemetry);
#[cfg(target_os = "macos")]
impl std::ops::Deref for DeliverySession {
    type Target = beatkernel::telemetry::InputDeliveryTelemetry;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
#[cfg(target_os = "macos")]
impl std::ops::DerefMut for DeliverySession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
#[cfg(target_os = "macos")]
impl Drop for DeliverySession {
    fn drop(&mut self) {
        let observed = self.observed_events();
        match self.summary() {
            Some(summary) => println!(
                "IOHID-event-to-runtime delivery age: observed_events={observed}; retained samples={} p50={}ns p95={}ns p99={}ns max={}ns; HOST={:?}, capacity={}; separate from CPU processing; physical input-to-sound unknown",
                summary.samples,
                summary.p50_ns,
                summary.p95_ns,
                summary.p99_ns,
                summary.max_ns,
                self.domain(),
                self.capacity()
            ),
            None => println!(
                "IOHID-event-to-runtime delivery age: observed_events={observed}; retained summary unavailable; no zero observation substituted; separate from CPU processing; physical input-to-sound unknown"
            ),
        }
    }
}

#[cfg(target_os = "macos")]
use beatkernel_bms_runtime::native_finish::{finish_solo_with_result_and_score, save_capture};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    run_args(&args)
}

fn finite_mode(options: &Options, _network: bool) -> Result<()> {
    options.playback_end()?;
    Ok(())
}
#[cfg(any(target_os = "macos", test))]
#[cfg(test)]
fn finite_session_done(
    end: Option<i64>,
    presented: Option<ClockPoint>,
    frontier: ClockPoint,
    song: Timestamp,
    backlog: bool,
    resuming: bool,
) -> bool {
    end.is_some_and(|end| song.as_nanos() >= end)
        && presented.is_some_and(|boundary| {
            boundary.domain == frontier.domain && frontier.timestamp >= boundary.timestamp
        })
        && !backlog
        && !resuming
}
#[cfg(any(target_os = "macos", test))]
#[cfg(test)]
fn before_finite_end(input: ClockPoint, presented: Option<ClockPoint>) -> Result<bool> {
    if let Some(boundary) = presented {
        if input.domain != boundary.domain {
            return Err("CoreAudio terminal boundary/input clock domain differs".into());
        }
        return Ok(input.timestamp < boundary.timestamp);
    }
    Ok(true)
}

/// Validate settings through the same parsers as play, without opening any resources.
#[allow(dead_code)] // Standalone native binaries have no settings screen.
pub(crate) fn validate_args(args: &[String]) -> Result<()> {
    let (competition, native) =
        beatkernel_bms_runtime::competition_live::CompetitionOptions::extract(args)?;
    let options = parse(&native)?;
    beatkernel_bms_runtime::native_judge::validate_policy_competition(options.gauge, &competition)?;
    finite_mode(&options, competition.network.is_some())?;
    Ok(())
}

pub(crate) fn run_args(args: &[String]) -> Result<()> {
    let (competition_options, args) =
        beatkernel_bms_runtime::competition_live::CompetitionOptions::extract(args)?;
    if args.is_empty() || args == ["--help"] {
        println!(
            "Graphical player is bms-player; this is a native developer composition. Local mode: replace --keyboard-registry with repeated --local-player ID:REGISTRY (2..64 distinct keyboards). Network local groups share one connection and start agreement.\n"
        );
        println!(
            "macos_bms --chart PATH --device AUDIO_DEVICE_ID --keyboard-registry IOREGISTRY_ENTRY_ID --rate HZ --channels N --buffer-frames N [--seconds N] --bind channelHEX:HIDusageHEX [--bind ...]\nOptions: --record-replay PATH --replay-max-records N --replay-max-bytes N --early-ns N --late-ns N --input-offset-ns N --chart-seed DECIMAL_U64 --gauge beatkernel|assist-easy|easy|groove|hard|ex-hard|hazard --start-ns N --end-ns N --preroll-ns N --bgm-lookahead-ns N --advance-lag-ns N --voices N --channel-policy exact|mono-stereo\nGauge timing: existing early/late window gives one PGREAT hit class and POOR misses with input offset; full LR2 judgment windows are not provided. Saved ghosts must match the chosen policy. Nondefault multiplayer remains unavailable.\nBounds: start unsigned0..9223372036854775807ns, BGM lookahead positive i64 ns, seconds 1..3600, preroll 0..10000000000 ns, advance lag 0..1000000000 ns, voices 1..4096. Defaults: gauge beatkernel, chart seed0, replay disabled, max records 1000000, max bytes 67108864, BGM lookahead3000000000ns, windows 150000000 ns, offset 0 ns, preroll 3000000000 ns, advance lag 2000000 ns, voices 256, exact channels. Optional --end-ns is unsigned and strictly after start; solo or local cohort CoreAudio completes a finite prefix only after native presentation and input drain, without forcing remaining notes. Network peers must agree on the same finite section endpoint. Missing --seconds plays the full song through terminal judging and reported native audio presentation; --seconds is a diagnostic loop cutoff after startup. Exact solo or assigned local registry attachments, actual keyboard HID controls; native float32 CoreAudio, no fallback. Physical timing Unknown."
        );
        return Ok(());
    }
    let options = parse(&args)?;
    beatkernel_bms_runtime::native_judge::validate_policy_competition(
        options.gauge,
        &competition_options,
    )?;
    finite_mode(&options, competition_options.network.is_some())?;
    #[cfg(target_os = "macos")]
    {
        native::run(options, competition_options)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (options, competition_options);
        Err("macos_bms native playback requires macOS".into())
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use beatkernel::{
        audio::PcmLimits,
        input::{Binding, BindingMap, DeviceSelector, GameControlId, PhysicalControlId},
        time::{ClockDomainId, Duration},
        transport::{Rate, Transport},
    };
    use beatkernel_bms_runtime::local_runtime::SoloRuntime as Runtime;
    use beatkernel_bms_runtime::native_audio::{
        prepare_audio, prepare_input_sounds, prepare_mine_sounds, NativeAudioConfig,
        PreparedNativeAudio,
    };
    use beatkernel_bms_runtime::{
        native_chart::{prepare_chart, NativeChartConfig},
        native_end::NativeEnd,
        native_judge::{capture_limits, prepare_section_capture_for_policy, NativeJudgeConfig},
        playback_pause::NativePause,
        player::{self},
        ChannelPolicy,
    };
    use beatkernel_platform::{
        audio::presentation::discipline::{DisciplineConfig, PresentationDiscipline},
        macos::{
            audio::{CoreAudioRequest, CoreAudioStream},
            clock::MachClock,
            presentation::coreaudio_presentation_pair,
        },
    };
    use std::{
        collections::VecDeque,
        time::Duration as WallDuration,
    };
    pub(super) const M_NATIVE: ClockDomainId = ClockDomainId(1);
    pub(super) const HOST: ClockDomainId = ClockDomainId(2);
    pub(super) const OUTPUT: ClockDomainId = ClockDomainId(3);
    pub(super) const LOGICAL: ClockDomainId = ClockDomainId(4);

    pub(super) fn output_origin() -> ClockPoint {
        ClockPoint {
            domain: OUTPUT,
            timestamp: Timestamp::ZERO,
        }
    }
    pub(super) fn observe(audio: &CoreAudioStream, clock: &MachClock) -> Result<Option<ClockPair>> {
        let snapshot = audio.snapshot();
        if snapshot.configuration_changed || snapshot.callback_failures != 0 {
            return Err(format!("CoreAudio configuration/callback failure: {snapshot:?}").into());
        }
        let Some(presentation) = snapshot.presentation else {
            return Ok(None);
        };
        Ok(coreaudio_presentation_pair(
            presentation,
            audio.configuration(),
            HOST,
            clock,
        )?)
    }
    use beatkernel_bms_runtime::native_start::{
        start_committed, NativeStartConfig, NativeStartDevice, NativeStartObservation,
        NativeStartResult,
    };
    pub(super) struct StartupDevice<'a> {
        pub(super) audio: &'a mut CoreAudioStream,
        pub(super) input: &'a mut super::collected_input::Collector,
        pub(super) clock: &'a MachClock,
        pub(super) pre_origin: &'a mut u64,
        pub(super) retained: &'a mut beatkernel_bms_runtime::native_gameplay::NativeCollectedInput,
    }
    impl NativeStartDevice for StartupDevice<'_> {
        type Evidence = ();
        fn start(&mut self) -> NativeStartResult<()> {
            Ok(self.audio.start()?)
        }
        fn service_input(&mut self, retain: bool) -> NativeStartResult<bool> {
            startup_input(
                self.input,
                self.pre_origin,
                &mut *self.retained,
                retain,
            )
        }
        fn observe(&mut self) -> NativeStartResult<Option<NativeStartObservation<()>>> {
            Ok(
                observe(self.audio, self.clock)?.map(|pair| NativeStartObservation {
                    timing: pair.into(),
                    evidence: (),
                }),
            )
        }
        fn render_report(&mut self) -> NativeStartResult<Option<beatkernel::audio::RenderReport>> {
            Ok(self.audio.last_render_report())
        }
        fn buffer_frames(&self) -> NativeStartResult<u32> {
            Ok(self.audio.configuration().buffer_frames)
        }
        fn host_now(&self) -> NativeStartResult<ClockPoint> {
            Ok(self.clock.sample()?.normalized)
        }
    }
    use beatkernel_bms_runtime::native_gameplay::{
        run_gameplay_audio_with_policy_and_result_and_score, InputBatch,
        NativeAudioGameplaySession, NativeGameplayConfig, NativeGameplayDevice,
        NativeGameplayResult,
    };
    type OwnedOutput = beatkernel_bms_runtime::gameplay::output::adapters::coreaudio_ui::NativeCoreAudioOutputOwner;
    struct GameplayDevice<'a> {
        output: &'a mut OwnedOutput,
        output_ui: &'a mut beatkernel_bms_runtime::gameplay::output::adapters::coreaudio_ui::NativeCoreAudioOutputUi,
        input: &'a mut super::collected_input::Collector,
        clock: &'a MachClock,
        retained: &'a mut beatkernel_bms_runtime::native_gameplay::NativeCollectedInput,
    }
    impl NativeGameplayDevice for GameplayDevice<'_> {
        fn observe_audio(
            &mut self,
            presentation: &mut beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
        ) -> NativeGameplayResult<()> {
            self.input.status()?;
            Ok(self.output.observe_native(presentation)?)
        }
        fn audio_pause_observation(
            &mut self,
            presentation: &beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
            now: ClockPoint,
        ) -> NativeGameplayResult<beatkernel_bms_runtime::live_pause::LivePauseObservation>
        {
            Ok(self.output.audio_pause_observation(presentation, now)?)
        }
        fn observe_audio_end(
            &mut self,
            end: &mut NativeEnd,
            presentation: &beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
            _rendered: Option<beatkernel::audio::RenderReport>,
        ) -> NativeGameplayResult<Option<beatkernel_bms_runtime::native_end::EndBoundary>> {
            self.output.observe_audio_end(end, presentation)
        }
        fn publish_paused_audio_output(
            &mut self,
            context: beatkernel_bms_runtime::gameplay_presentation::GameplayAudioOutputContext<'_>,
            now: ClockPoint,
        ) -> NativeGameplayResult<bool> {
            if !self.output.has_work() && !self.output_ui.pending() {
                return Ok(false);
            }
            self.output_ui.service_audio(self.output, context, now)
        }
        fn observe(&mut self, discipline: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
            self.input.status()?;
            self.output.observe(discipline)?;
            Ok(())
        }
        fn output_clock_suspended(&self) -> bool {
            self.output.output_clock_suspended()
        }
        fn output_replacement_pending(&self) -> bool {
            self.output.replacement_pending() || self.output_ui.pending()
        }
        fn publish_paused_output(
            &mut self,
            context: beatkernel_bms_runtime::gameplay_presentation::GameplayOutputContext<
                '_,
                PresentationDiscipline,
            >,
        ) -> NativeGameplayResult<bool> {
            if !self.output.has_work() && !self.output_ui.pending() {
                return Ok(false);
            }
            self.output_ui
                .service(self.output, context, self.clock.sample()?.normalized)
        }
        fn pause_observation(
            &mut self,
            pair: ClockPair,
        ) -> NativeGameplayResult<beatkernel_bms_runtime::live_pause::LivePauseObservation>
        {
            Ok(self.output.pause_observation(pair, pair.target)?)
        }
        fn render_report(
            &mut self,
        ) -> NativeGameplayResult<Option<beatkernel::audio::RenderReport>> {
            Ok(self.output.render_report())
        }
        fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
            Ok(self.clock.sample()?.normalized)
        }
        fn acquire(
            &mut self,
            events: &mut VecDeque<beatkernel::input::PhysicalInputEvent>,
        ) -> NativeGameplayResult<InputBatch> {
            self.input.activate();
            self.retained.acquire(&mut self.input.worker, events, 256)
        }
        fn observe_end(
            &mut self,
            end: &mut NativeEnd,
            discipline: &PresentationDiscipline,
            report: Option<beatkernel::audio::RenderReport>,
        ) -> NativeGameplayResult<Option<beatkernel_bms_runtime::native_end::EndBoundary>> {
            let _ = report;
            self.output.observe_end(end, discipline)
        }
        fn seed_resume(
            &mut self,
            discipline: &mut PresentationDiscipline,
            reference: ClockPair,
        ) -> NativeGameplayResult<()> {
            let _ = reference;
            Ok(self.output.seed_resume(discipline)?)
        }
        fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
            Err("CoreAudio uses logical mixer scheduling".into())
        }
    }
    pub(super) fn startup_input(
        input: &mut super::collected_input::Collector,
        pre_origin: &mut u64,
        retained: &mut beatkernel_bms_runtime::native_gameplay::NativeCollectedInput,
        retain: bool,
    ) -> Result<bool> {
        if player::cancelled() { return Ok(false); }
        input.activate();
        retained.service_start(&mut input.worker, retain, pre_origin, 256)
    }
    pub(super) struct AudioSeedDevice<'a> {
        pub(super) output: &'a mut OwnedOutput,
        pub(super) input: &'a mut super::collected_input::Collector,
        pub(super) clock: &'a MachClock,
        pub(super) pre_origin: &'a mut u64,
        pub(super) retained: &'a mut beatkernel_bms_runtime::native_gameplay::NativeCollectedInput,
        pub(super) end: Option<&'a mut NativeEnd>,
        pub(super) end_primed: bool,
    }
    impl beatkernel_bms_runtime::native_audio_startup::NativeAudioSeedPort for AudioSeedDevice<'_> {
        fn service_input(&mut self) -> NativeGameplayResult<bool> {
            startup_input(
                self.input,
                self.pre_origin,
                &mut *self.retained,
                true,
            )
        }
        fn observe_audio(
            &mut self,
            presentation: &mut beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
        ) -> NativeGameplayResult<()> {
            self.output.observe_native(presentation)?;
            if !self.end_primed {
                if let (Some(end), Some(record)) =
                    (self.end.as_deref_mut(), presentation.latest_record())
                {
                    let report = self
                        .output
                        .current()
                        .ok_or("CoreAudio startup output missing")?
                        .stream()
                        .last_render_report()
                        .ok_or("CoreAudio startup observation lacks its real render report")?;
                    end.prime(Some(report), record.pair())?;
                    self.end_primed = true;
                }
            }
            Ok(())
        }
        fn render_report(
            &mut self,
        ) -> NativeGameplayResult<Option<beatkernel::audio::RenderReport>> {
            let stream = self
                .output
                .current()
                .ok_or("CoreAudio startup output missing")?
                .stream();
            let snapshot = stream.snapshot();
            if snapshot.configuration_changed
                || snapshot.callback_failures != 0
                || !stream.is_started()
            {
                return Err(format!("CoreAudio startup stream failed: {snapshot:?}").into());
            }
            Ok(stream.last_render_report())
        }
        fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
            Ok(self.clock.sample()?.normalized)
        }
        fn wait(&mut self, duration: WallDuration) -> NativeGameplayResult<()> {
            std::thread::sleep(duration);
            Ok(())
        }
    }
    pub(super) fn audio_startup_bounds(
        stream: &CoreAudioStream,
    ) -> Result<(
        Duration,
        beatkernel_bms_runtime::audio_authority::AudioAuthorityConfig,
    )> {
        let config = stream.configuration();
        let rate = u128::from(config.format.sample_rate());
        if rate == 0 || config.buffer_frames == 0 {
            return Err(
                "CoreAudio startup requires a positive negotiated rate and callback buffer".into(),
            );
        }
        let quantum = (u128::from(config.buffer_frames) * 1_000_000_000).div_ceil(rate);
        let timeout = Duration::from_nanos(i64::try_from((quantum * 4).max(2_000_000_000))?);
        let max_age = Duration::from_nanos(i64::try_from((quantum * 3).max(1_000_000_000))?);
        Ok((
            timeout,
            beatkernel_bms_runtime::audio_authority::AudioAuthorityConfig {
                max_observation_age: max_age,
                ..Default::default()
            },
        ))
    }
    pub(super) fn run(
        options: Options,
        competition_options: beatkernel_bms_runtime::competition_live::CompetitionOptions,
    ) -> Result<()> {
        if !options.local_players.is_empty() {
            return super::local_native::run(options, competition_options);
        }
        let playback_end = options.playback_end()?;
        let song_origin = options.song_origin()?;
        let mut pause = NativePause::new(output_origin(), HOST, options.format.sample_rate())?;
        if let Some(end) = playback_end {
            pause = pause.with_playback_end_frame(end)?;
        }
        let mut native_end = playback_end
            .map(|end| NativeEnd::new(output_origin(), HOST, options.format.sample_rate(), end))
            .transpose()?;
        let clock = MachClock::new(M_NATIVE, HOST)?;
        let network_start = competition_options.network.is_some();
        let pause_supported = !network_start;
        // Declared before device owners so every exit reports after their cleanup.
        let mut delivery = DeliverySession(beatkernel::telemetry::InputDeliveryTelemetry::new(
            4096, HOST,
        )?);
        let (prepared, section) = prepare_chart(NativeChartConfig {
            path: &options.chart,
            format: options.format,
            limits: PcmLimits::new(
                64 * 1024 * 1024,
                256 * 1024 * 1024,
                beatkernel_bms_runtime::DEFAULT_BMS_PCM_SAMPLES,
            )?,
            channels: if options.mono_stereo {
                ChannelPolicy::MonoToStereo
            } else {
                ChannelPolicy::Exact
            },
            chart_seed: options.chart_seed,
            start: Timestamp::from_nanos(options.start_ns),
            bindings: &options.bindings,
        })?;
        println!("prepared practice section={section:?}");
        let input_sounds = prepare_input_sounds(&prepared)?;
        let hazard_sounds = prepare_mine_sounds(&prepared, input_sounds.as_ref())?;
        let judge_config = NativeJudgeConfig {
            early: options.early,
            late: options.late,
            offset: options.offset,
            preroll: options.preroll,
            output: OUTPUT,
            end: options.end_ns.map(Timestamp::from_nanos),
        };
        let policy = judge_config.resolve_play_policy(&section.original_gauge, options.gauge)?;
        let mut gauge = beatkernel_bms_runtime::gauge::BmsGauge::new(policy.gauge().try_copy()?);
        let mut completion = judge_config.completion(&prepared)?;
        for warning in &prepared.source.warnings {
            eprintln!("BMS warning line {}: {}", warning.line, warning.message);
        }
        beatkernel_bms_runtime::player::publish_native_chart(
            &options.chart,
            &prepared.source,
            &prepared.compiled.chart,
            &[beatkernel_bms_runtime::local_players::PlayerId(1)],
        )?;
        if beatkernel_bms_runtime::player::cancelled() {
            return Ok(());
        }
        let judge =
            judge_config.judge_with_policy(&prepared.source, prepared.compiled.chart, &policy)?;
        let mut competition =
            beatkernel_bms_runtime::competition_live::LiveCompetition::prepare_native_section_with_policy(
                &competition_options,
                &prepared.source,
                &judge,
                &policy,
                LOGICAL,
                Timestamp::from_nanos(options.start_ns),
                options.chart_seed,
                options.end_ns.map(Timestamp::from_nanos),
                options.preroll,
            )?;
        const SLACK: usize = beatkernel_bms_runtime::native_audio::LIVE_COMMAND_RESERVE;
        let capacity = AudioLimits::MAX_COMMANDS;
        let PreparedNativeAudio {
            mut producer,
            bgm,
            mixer,
        } = prepare_audio(
            prepared.bank,
            prepared.bgm_commands,
            NativeAudioConfig {
                output_origin: output_origin(),
                start: Timestamp::from_nanos(options.start_ns),
                preroll: Duration::from_nanos(options.preroll),
                lookahead: Duration::from_nanos(options.bgm_lookahead),
                voices: options.voices,
                max_render_frames: options.buffer as usize,
                playback_end_frame: playback_end,
                gated_start: network_start,
            },
        )?;
        let mut bgm = BgmSession(bgm);
        let (mut input, mut selected_devices) = super::collected_input::open(clock, vec![options.keyboard_registry], 1024)?;
        let selected = selected_devices.remove(0);
        let selected_id = selected.descriptor.runtime_id;
        let bindings =
            BindingMap::from_bindings(options.bindings.iter().map(|(&channel, &key)| Binding {
                device: DeviceSelector::Exact(selected_id),
                physical: PhysicalControlId::keyboard(key),
                game_control: GameControlId(u32::from(channel)),
            }))?;
        let request = CoreAudioRequest {
            device: options.device,
            format: options.format,
            buffer_frames: options.buffer,
        };
        let audio = match CoreAudioStream::open(request, clock, mixer) {
            Ok(audio) => audio,
            Err(error) => {
                if let Err(close) = input.close() {
                    eprintln!("IOHID close error after audio open failure: {close}");
                }
                return Err(error.into());
            }
        };
        println!(
            "requested/applied CoreAudio={:?}; exact selected attachment={selected:?}; explicit keyboard bindings={:?}; windows={}/{}ns offset={}ns preroll={}ns advance_lag={}ns voices={} channels={} queue/pending={} live_slack={SLACK}",
            audio.configuration(),
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
        use beatkernel_bms_runtime::gameplay::output::adapters::coreaudio_ui::{
            owner, NativeCoreAudioOutputUi,
        };
        let mut output = owner(audio, clock, HOST);
        let mut output_ui = NativeCoreAudioOutputUi::new(&output, !network_start)?;
        let mut pre_origin = 0u64;
        let mut capture = None;
        let mut startup_inputs = beatkernel_bms_runtime::native_gameplay::NativeCollectedInput::new()?;
        let mut score = beatkernel_bms_runtime::competition::ScoreSummary::default();
        let outcome =
            (|| -> Result<Option<beatkernel_bms_runtime::play_result::CompletedPlayResult>> {
                capture = prepare_section_capture_for_policy(
                    &prepared.source,
                    &judge,
                    &policy,
                    LOGICAL,
                    Timestamp::from_nanos(options.start_ns),
                    options.chart_seed,
                    options.end_ns.map(Timestamp::from_nanos),
                    capture_limits(
                        options.record_replay.is_some(),
                        options.replay_max_bytes,
                        options.replay_max_records,
                    )?,
                )?;
                input.status()?;
                let (network_origin, playback_origin) = if network_start {
                    let competition = competition
                        .as_mut()
                        .ok_or("network startup owner missing")?;
                    let started = {
                        let mut device = StartupDevice {
                            audio: output
                                .current_mut()
                                .ok_or("initial CoreAudio output missing")?
                                .stream_mut(),
                            input: &mut input,
                            clock: &clock,
                            pre_origin: &mut pre_origin,
                            retained: &mut startup_inputs,
                        };
                        start_committed(
                            &mut device,
                            competition,
                            &mut producer,
                            &mut pause,
                            &mut native_end,
                            NativeStartConfig {
                                output_origin: output_origin(),
                                sample_rate: options.format.sample_rate(),
                                playback_end_frame: playback_end,
                                setup_timeout: competition_options.setup_timeout,
                                max_clock_age_ns: competition_options.start_policy.max_age_ns,
                                max_rate_error_ppm: DisciplineConfig::default().max_rate_error_ppm,
                            },
                            |report, producer| {
                                feed_rendered(&mut bgm, report, |command| {
                                    producer.try_push(command)
                                })
                            },
                        )?
                    };
                    let Some(started) = started else {
                        return Ok(None);
                    };
                    println!(
                        "native applied start={:?}; HOST window={:?}; physical accuracy unmeasured",
                        started.plan, started.host_window
                    );
                    (Some(started.host_origin), started.plan.selected_output())
                } else {
                    output
                        .current_mut()
                        .ok_or("initial CoreAudio output missing")?
                        .stream_mut()
                        .start()?;
                    (None, output_origin())
                };
                let current = output.current().ok_or("initial CoreAudio output missing")?;
                let (startup_timeout, authority_config) = audio_startup_bounds(current.stream())?;
                let mut presentation =
                    beatkernel_bms_runtime::native_audio_startup::new_audio_presentation(
                        current.epoch(),
                        current.stream().frame_basis(),
                        HOST,
                        ClockPoint {
                            domain: LOGICAL,
                            timestamp: Timestamp::ZERO,
                        },
                        authority_config,
                    )?;
                let seeded = {
                    let mut seed = AudioSeedDevice {
                        output: &mut output,
                        input: &mut input,
                        clock: &clock,
                        pre_origin: &mut pre_origin,
                        retained: &mut startup_inputs,
                        end: if network_start {
                            None
                        } else {
                            native_end.as_mut()
                        },
                        end_primed: false,
                    };
                    beatkernel_bms_runtime::native_audio_startup::prime_native_audio(
                        &mut seed,
                        &mut presentation,
                        &mut bgm.0,
                        &mut producer,
                        startup_timeout,
                    )?
                };
                let Some(seeded) = seeded else {
                    return Ok(None);
                };
                let origin = match network_origin {
                    Some(origin) => origin,
                    None => seeded.host_for_output(playback_origin, startup_timeout)?,
                };
                let logical_origin = presentation.logical_output(playback_origin)?;
                let transport = Transport::new(logical_origin.timestamp, song_origin, Rate::NORMAL);
                let mut merger = beatkernel_bms_runtime::local_input::InputMerger::new_dynamic(
                    HOST, origin, 4096, 65536,
                )?;
                merger.register_source(selected_id)?;
                println!("audio authority original anchors={:?}; acquisition origin={origin:?}; physical latency unmeasured", seeded.observations);
                if options.preroll == 0 {
                    println!("zero preroll permits startup consumption of initial BGM/notes");
                }
                let mut runtime = Runtime::new(
                    LOGICAL,
                    OUTPUT,
                    transport,
                    bindings,
                    judge,
                    producer,
                    prepared.sounds,
                    4096,
                )?;
                if let Some(timeline) = input_sounds {
                    runtime.configure_input_sounds(timeline)?;
                }
                if let Some(timeline) = hazard_sounds {
                    runtime.configure_hazard_sounds(timeline)?;
                }
                if let Some(end) = options.end_ns {
                    runtime.set_song_end(Timestamp::from_nanos(end))?;
                }
                let pump = {
                    let mut device = GameplayDevice {
                        output: &mut output,
                        output_ui: &mut output_ui,
                        input: &mut input,
                        clock: &clock,
                        retained: &mut startup_inputs,
                    };
                    run_gameplay_audio_with_policy_and_result_and_score(
                        &mut device,
                        NativeAudioGameplaySession {
                            merger: &mut merger,
                            session: beatkernel_bms_runtime::native_gameplay::GameplaySession {
                                runtime: &mut runtime,
                                gauge: &mut gauge,
                                bgm: &mut bgm,
                                discipline: &mut presentation,
                                pause: &mut pause,
                                end: &mut native_end,
                                completion: &mut completion,
                                capture: &mut capture,
                                competition: &mut competition,
                                delivery: &mut delivery,
                                pre_origin_inputs: &mut pre_origin,
                            },
                        },
                        beatkernel_bms_runtime::native_gameplay::AudioGameplayConfig {
                            section_start: Timestamp::from_nanos(options.start_ns),
                            gameplay: NativeGameplayConfig {
                                origin,
                                stream_origin: output_origin(),
                                playback_origin,
                                song_origin,
                                sample_rate: options.format.sample_rate(),
                                end_song: options.end_ns.map(Timestamp::from_nanos),
                                advance_lag: Duration::from_nanos(options.advance_lag),
                                seconds: options.seconds,
                                pause_supported,
                                logical_schedule: true,
                            },
                        },
                        &mut score,
                        &policy,
                    )
                };
                println!(
                    "runtime processing={:?} counters={:?}; pre-origin ignored={pre_origin}",
                    runtime.telemetry().processing(),
                    runtime.telemetry().counters()
                );
                pump
            })();
        let stop = output.stop();
        let close = input.close();
        println!(
            "final CoreAudio native snapshot={:?}; input collector status={:?}; pre-origin ignored={pre_origin}; physical latency unmeasured",
            output.current().map(|out| out.stream().snapshot()),
            input.worker.status()
        );
        match output
            .current()
            .and_then(|out| out.stream().last_render_report())
        {
            Some(report) => println!(
                "last successful typed core RenderReport={report:?}; core execution distinct from native delivery/physical sound"
            ),
            None => println!(
                "last successful core RenderReport unavailable; no zero observation substituted"
            ),
        }
        if let Err(error) = &stop {
            eprintln!("CoreAudio stop error (existing context-retention policy): {error}");
        }
        if let Err(error) = &close {
            eprintln!("IOHID close error: {error}");
        }
        finish_solo_with_result_and_score(
            outcome,
            stop.map_err(Into::into),
            close.map_err(Into::into),
            competition.as_mut(),
            capture,
            gauge.profile(),
            &score,
            options.record_replay.as_deref(),
            save_capture,
            |archive, path| {
                beatkernel_bms_runtime::native_result_archive::save_sidecar(
                    archive,
                    path.ok_or("completed archive missing base replay path")?,
                )
            },
        )
    }
}

#[cfg(target_os = "macos")]
#[path = "macos_bms/local.rs"]
mod local_native;

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::{
        audio::{SampleId, VoiceId},
        time::ClockDomainId,
    };
    fn point(n: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(2),
            timestamp: Timestamp::from_nanos(n),
        }
    }
    #[test]
    fn finite_cli_checks_exact_rate_mapping_unsigned_end_and_unsupported_modes() {
        assert_eq!(parse(&args()).unwrap().playback_end().unwrap(), None);
        let mut configured = args();
        configured.extend([
            "--start-ns".into(),
            "72000000000000".into(),
            "--end-ns".into(),
            "72000001000001".into(),
        ]);
        for (rate, expected) in [(44100, 132345), (48000, 144049)] {
            configured[7] = rate.to_string();
            let options = parse(&configured).unwrap();
            assert_eq!(options.playback_end().unwrap(), Some(expected));
            assert!(validate_args(&configured).is_ok());
        }
        for value in ["", "-1", "+1", "1.5", "1e9", "9223372036854775808", "0"] {
            let mut invalid = args();
            invalid.extend(["--end-ns".into(), value.into()]);
            assert!(parse(&invalid).is_err());
        }
        for end in ["9", "10"] {
            let mut invalid = args();
            invalid.extend([
                "--start-ns".into(),
                "10".into(),
                "--end-ns".into(),
                end.into(),
            ]);
            assert!(parse(&invalid).is_err());
        }
        let mut duplicate = args();
        duplicate.extend(["--end-ns".into(), "1".into(), "--end-ns".into(), "2".into()]);
        assert!(parse(&duplicate).is_err());
        let mut network = args();
        network.extend([
            "--end-ns".into(),
            "1000000".into(),
            "--mp-host".into(),
            "127.0.0.1:34567".into(),
        ]);
        assert!(validate_args(&network).is_ok());
        let mut local = args();
        local.drain(4..6);
        local.extend([
            "--local-player".into(),
            "7:100".into(),
            "--local-player".into(),
            "4294967295:200".into(),
            "--end-ns".into(),
            "1000000".into(),
        ]);
        assert!(parse(&local).is_ok());
        assert!(validate_args(&local).is_ok());
        local.extend(["--mp-host".into(), "127.0.0.1:34567".into()]);
        assert!(validate_args(&local).is_ok());
    }
    #[test]
    fn finite_frontier_is_exclusive_and_requires_real_drain_resume_and_logical_end() {
        assert!(before_finite_end(point(9), Some(point(10))).unwrap());
        assert!(!before_finite_end(point(10), Some(point(10))).unwrap());
        assert!(!before_finite_end(point(11), Some(point(10))).unwrap());
        assert!(before_finite_end(point(11), None).unwrap());
        let wrong = ClockPoint {
            domain: ClockDomainId(3),
            timestamp: Timestamp::from_nanos(10),
        };
        assert!(before_finite_end(wrong, Some(point(10))).is_err());
        assert!(finite_session_done(
            Some(10),
            Some(point(20)),
            point(20),
            Timestamp::from_nanos(10),
            false,
            false
        ));
        for (end, boundary, frontier, song, backlog, resuming) in [
            (None, Some(point(20)), point(20), 10, false, false),
            (Some(10), None, point(20), 10, false, false),
            (Some(10), Some(point(20)), point(19), 10, false, false),
            (Some(10), Some(wrong), point(20), 10, false, false),
            (Some(10), Some(point(20)), point(20), 9, false, false),
            (Some(10), Some(point(20)), point(20), 10, true, false),
            (Some(10), Some(point(20)), point(20), 10, false, true),
        ] {
            assert!(!finite_session_done(
                end,
                boundary,
                frontier,
                Timestamp::from_nanos(song),
                backlog,
                resuming
            ));
        }
    }
    #[test]
    fn actual_finite_mixer_short_resume_waits_native_end_and_collector_release_order() {
        use beatkernel::{
            audio::*,
            input::{
                ButtonEvent, ButtonState, DeviceId, EventMeta, PhysicalControlId,
                PhysicalInputEvent,
            },
        };
        use beatkernel_bms_runtime::{
            native_end::NativeEnd,
            playback_pause::{NativePause, PauseKeyboard, PausePhase},
        };
        let output = |ns| ClockPoint {
            domain: ClockDomainId(3),
            timestamp: Timestamp::from_nanos(ns),
        };
        let pair = |ns| ClockPair {
            source: output(ns),
            target: point(ns),
        };
        let format = AudioFormat::new(1000, 1).unwrap();
        let limits = AudioLimits::new(8, 2, 8, 32, 8).unwrap();
        let pcm = PcmLimits::new(4096, 8192, 2).unwrap();
        let mut bank = SampleBank::new(format, pcm).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.25; 16], pcm).unwrap(),
        )
        .unwrap();
        let (mut producer, consumer) = command_queue(8).unwrap();
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.0,
            })
            .unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(format, ClockDomainId(3), Timestamp::ZERO, limits)
                .with_playback_end_frame(4),
            bank,
            consumer,
        )
        .unwrap();
        let mut pause = NativePause::new(output(0), ClockDomainId(2), 1000)
            .unwrap()
            .with_playback_end_frame(4)
            .unwrap();
        let mut end = NativeEnd::new(output(0), ClockDomainId(2), 1000, 4).unwrap();
        end.observe(None, pair(0)).unwrap();
        let active = mixer.render(&mut [0.0; 2]).unwrap();
        pause.observe(Some(active), pair(1_000_000)).unwrap();
        end.observe(Some(active), pair(1_000_000)).unwrap();
        pause.request(true, pair(1_000_000)).unwrap();
        producer.request_pause(true);
        let paused = mixer.render(&mut [0.0; 3]).unwrap();
        pause
            .observe(Some(paused), pair(3_000_000))
            .unwrap()
            .unwrap();
        end.observe(Some(paused), pair(3_000_000)).unwrap();
        pause.request(false, pair(4_000_000)).unwrap();
        producer.request_pause(false);
        let prefix = mixer.render(&mut [0.0; 5]).unwrap();
        assert_eq!(prefix.playback_frames, 2);
        assert_eq!(prefix.playback_end_physical_frame, Some(7));
        let latest = mixer.render(&mut [0.0; 5]).unwrap();
        assert_eq!(pause.observe(Some(latest), pair(4_500_000)).unwrap(), None);
        assert_eq!(end.observe(Some(latest), pair(4_500_000)).unwrap(), None);
        let resume = pause.observe(None, pair(6_000_000)).unwrap().unwrap();
        assert_eq!(resume.host, point(5_000_000));
        assert_eq!(pause.phase(), PausePhase::Running);
        assert_eq!(end.observe(Some(latest), pair(6_000_000)).unwrap(), None);
        let terminal = end.observe(Some(latest), pair(8_000_000)).unwrap().unwrap();
        assert_eq!(terminal.host, point(7_000_000));
        let event = |state, ns, seq| {
            PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(DeviceId(1000), point(ns), seq),
                control: PhysicalControlId::keyboard(4),
                state,
            })
        };
        let mut keyboard = PauseKeyboard::new();
        keyboard
            .accept(&event(ButtonState::Down, 1_000_000, 1))
            .unwrap();
        keyboard
            .observe_paused(event(ButtonState::Up, 4_000_000, 2))
            .unwrap();
        let mut parked = Vec::new();
        for (ns, seq) in [(6_000_000, 3), (7_000_000, 4)] {
            assert_eq!(
                pause_input_stage(point(ns), None, Some(resume.host)).unwrap(),
                PauseInputStage::AfterResume
            );
            park_resume_event(&mut parked, event(ButtonState::Down, ns, seq)).unwrap();
        }
        assert!(!finite_session_done(
            Some(4_000_000),
            Some(terminal.host),
            point(8_000_000),
            Timestamp::from_nanos(4_000_000),
            true,
            true
        ));
        let releases = keyboard.resume(resume.host).unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].meta().timestamp, resume.host.timestamp);
        assert_eq!(
            releases[0].meta().original_clock_point,
            Some(point(4_000_000))
        );
        let gameplay: Vec<_> = parked
            .into_iter()
            .filter(|event| {
                before_finite_end(
                    ClockPoint {
                        domain: event.meta().clock_domain,
                        timestamp: event.meta().timestamp,
                    },
                    Some(terminal.host),
                )
                .unwrap()
            })
            .collect();
        assert_eq!(gameplay.len(), 1);
        assert_eq!(gameplay[0].meta().timestamp, point(6_000_000).timestamp);
        assert!(finite_session_done(
            Some(4_000_000),
            Some(terminal.host),
            point(8_000_000),
            Timestamp::from_nanos(4_000_000),
            false,
            false
        ));
        assert_eq!(
            pause.song_origin_after_pause(Timestamp::ZERO).unwrap(),
            Timestamp::from_nanos(-3_000_000)
        );
    }
    #[test]
    fn chart_seed_is_shared_unsigned_u64_singleton_for_solo_and_local() {
        let base = args();
        assert_eq!(parse(&base).unwrap().chart_seed, 0);
        let mut local: Vec<String> = base
            .chunks_exact(2)
            .filter(|pair| pair[0] != "--keyboard-registry")
            .flat_map(|pair| pair.iter().cloned())
            .collect();
        local.extend([
            "--local-player".into(),
            "3:3".into(),
            "--local-player".into(),
            "4294967295:4294967295".into(),
        ]);
        assert_eq!(parse(&local).unwrap().chart_seed, 0);
        for original in [&base, &local] {
            for seed in [0_u64, 3, u64::MAX] {
                let mut configured = (*original).clone();
                configured.extend(["--chart-seed".into(), seed.to_string()]);
                let options = parse(&configured).unwrap();
                assert_eq!(options.chart_seed, seed);
                assert_eq!(options.start_ns, 0);
                assert_eq!(options.bindings[&0x11], 4);
                if original == &local {
                    assert_eq!(
                        options
                            .local_players
                            .iter()
                            .map(|player| player.0.0)
                            .collect::<Vec<_>>(),
                        vec![3, u32::MAX]
                    );
                }
            }
            for value in [
                "",
                "-1",
                "+1",
                "1.0",
                "1e2",
                " 3",
                "3 ",
                "18446744073709551616",
            ] {
                let mut invalid = (*original).clone();
                invalid.extend(["--chart-seed".into(), value.into()]);
                assert!(parse(&invalid).is_err(), "{value}");
            }
            let mut missing = (*original).clone();
            missing.push("--chart-seed".into());
            assert!(parse(&missing).is_err());
            let mut duplicate = (*original).clone();
            duplicate.extend([
                "--chart-seed".into(),
                "0".into(),
                "--chart-seed".into(),
                "3".into(),
            ]);
            assert!(parse(&duplicate).is_err());
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
    #[test]
    fn local_registry_assignments_preserve_ids_and_reject_aliases() {
        let solo = args();
        let base: Vec<String> = solo
            .chunks_exact(2)
            .filter(|pair| pair[0] != "--keyboard-registry")
            .flat_map(|pair| pair.iter().cloned())
            .collect();
        let mut group = base.clone();
        for value in [
            "1:100",
            "7:101",
            "99:102",
            "4294967295:18446744073709551615",
        ] {
            group.extend(["--local-player".into(), value.into()]);
        }
        let parsed = parse(&group).unwrap();
        assert_eq!(parsed.keyboard_registry, 0);
        assert_eq!(parsed.local_players.len(), 4);
        assert_eq!(parsed.local_players[3].0.0, u32::MAX);
        assert_eq!(parsed.local_players[3].1, u64::MAX);
        assert!(validate_args(&group).is_ok());
        let mut network = group.clone();
        network.extend(["--mp-host".into(), "127.0.0.1:34567".into()]);
        assert!(validate_args(&network).is_ok());
        for bad in [
            "0:4",
            "4:0",
            "+4:4",
            "4:+4",
            "4:4:5",
            "4294967296:4",
            "4:18446744073709551616",
            "1:500",
            "20:0100",
        ] {
            let mut invalid = group.clone();
            invalid.extend(["--local-player".into(), bad.into()]);
            assert!(parse(&invalid).is_err(), "{bad}");
        }
        let mut mixed = group.clone();
        mixed.extend(["--keyboard-registry".into(), "900".into()]);
        assert!(parse(&mixed).is_err());
        let mut single = base.clone();
        single.extend(["--local-player".into(), "1:100".into()]);
        assert!(parse(&single).is_err());
        let mut maximum = base;
        for id in 1..=64 {
            maximum.extend(["--local-player".into(), format!("{id}:{id}")]);
        }
        assert_eq!(parse(&maximum).unwrap().local_players.len(), 64);
        maximum.extend(["--local-player".into(), "65:65".into()]);
        assert!(parse(&maximum).is_err());
    }

    fn args() -> Vec<String> {
        [
            "--chart",
            "fixture.bms",
            "--device",
            "42",
            "--keyboard-registry",
            "900",
            "--rate",
            "48000",
            "--channels",
            "2",
            "--buffer-frames",
            "256",
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
    fn portable_cli_exact_ids_defaults_duplicates_and_bounds() {
        let parsed = parse(&args()).unwrap();
        assert_eq!(parsed.device, 42);
        assert_eq!(parsed.keyboard_registry, 900);
        assert_eq!(parsed.preroll, 3_000_000_000);
        assert_eq!(parsed.advance_lag, 2_000_000);
        assert_eq!(parsed.offset, 0);
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
        supplied[3] = "0".into();
        assert!(parse(&supplied).is_err());
        let mut supplied = args();
        supplied[5] = "0".into();
        assert!(parse(&supplied).is_err());
        let mut missing = args();
        missing.drain(10..12);
        assert!(parse(&missing).is_err()); // explicit buffer required
    }
    #[test]
    fn checked_bgm_shift_preserves_voice_asset_and_gain() {
        let command = AudioCommand::Play {
            voice: VoiceId(50000),
            sample: SampleId(12),
            at: Timestamp::from_nanos(100),
            gain: 0.5,
        };
        assert_eq!(shift_bgm(command, 0).unwrap(), command);
        assert_eq!(
            shift_bgm(command, 3_000_000_000).unwrap(),
            AudioCommand::Play {
                voice: VoiceId(50000),
                sample: SampleId(12),
                at: Timestamp::from_nanos(3_000_000_100),
                gain: 0.5
            }
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
    fn future_output_origin_defers_deadlines_without_clamping_native_pair() {
        let pair = ClockPair {
            source: ClockPoint {
                domain: ClockDomainId(3),
                timestamp: Timestamp::from_nanos(10),
            },
            target: point(1010),
        };
        let origin = estimated_origin(
            pair,
            ClockPoint {
                domain: ClockDomainId(3),
                timestamp: Timestamp::ZERO,
            },
        )
        .unwrap();
        assert_eq!(origin, Timestamp::from_nanos(1000));
        assert_eq!(pair.target, point(1010));
        assert_eq!(
            watermark(point(1000), point(1000), point(999), 0, false).unwrap(),
            None
        );
        assert_eq!(
            watermark(point(1000), point(1000), point(1001), 100, false).unwrap(),
            Some(point(1000))
        );
        assert!(estimated_origin(pair, point(0)).is_err());
        assert!(
            estimated_origin(
                ClockPair {
                    source: point(i64::MAX),
                    target: point(i64::MIN)
                },
                point(0)
            )
            .is_err()
        );
    }
    #[test]
    fn input_epoch_future_and_lag_backlog_guards_keep_original_time() {
        assert!(!input_in_epoch(point(9), point(20), point(10)).unwrap());
        assert!(input_in_epoch(point(10), point(20), point(10)).unwrap());
        assert!(input_in_epoch(point(21), point(20), point(10)).is_err());
        assert!(validate_input_chronology(point(100), point(100)).is_ok());
        assert!(validate_input_chronology(point(101), point(100)).is_ok());
        assert!(validate_input_chronology(point(99), point(100)).is_err());
        assert!(
            validate_input_chronology(
                ClockPoint {
                    domain: ClockDomainId(3),
                    timestamp: Timestamp::from_nanos(100)
                },
                point(100)
            )
            .is_err()
        );

        assert_eq!(
            watermark(point(10), point(100), point(200), 50, false).unwrap(),
            Some(point(150))
        );
        assert_eq!(
            watermark(point(10), point(100), point(200), 150, false).unwrap(),
            Some(point(100))
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
    }
    #[test]
    fn pause_sides_preserve_exact_boundaries_and_reject_changed_host_domains() {
        assert_eq!(
            pause_input_stage(point(9), Some(point(10)), None).unwrap(),
            PauseInputStage::Live
        );
        assert_eq!(
            pause_input_stage(point(10), Some(point(10)), None).unwrap(),
            PauseInputStage::Paused
        );
        assert_eq!(
            pause_input_stage(point(19), None, Some(point(20))).unwrap(),
            PauseInputStage::Paused
        );
        assert_eq!(
            pause_input_stage(point(20), None, Some(point(20))).unwrap(),
            PauseInputStage::AfterResume
        );
        assert!(
            pause_input_stage(
                point(20),
                None,
                Some(ClockPoint {
                    domain: ClockDomainId(99),
                    timestamp: Timestamp::from_nanos(20)
                })
            )
            .is_err()
        );
        assert_eq!(
            watermark(point(0), point(10), point(30), 0, true).unwrap(),
            None
        );
    }
    #[test]
    fn bounded_resume_parking_preserves_native_events_and_reconciles_releases_first() {
        use beatkernel::input::{
            BackendId, ButtonEvent, ButtonState, DeviceId, EventMeta, NativeEventMeta,
            PhysicalControlId, PhysicalInputEvent,
        };
        use beatkernel_bms_runtime::playback_pause::PauseKeyboard;
        let mut keyboard = PauseKeyboard::new();
        let event = |state, time, sequence| {
            let mut meta = EventMeta::new(DeviceId(1), point(time), sequence);
            meta.native = Some(NativeEventMeta {
                backend: BackendId(7),
                code: Some(4),
                timestamp: Some(point(time)),
            });
            PhysicalInputEvent::Button(ButtonEvent {
                meta,
                control: PhysicalControlId::keyboard(4),
                state,
            })
        };
        assert!(keyboard.accept(&event(ButtonState::Down, 1, 1)).unwrap());
        keyboard
            .observe_paused(event(ButtonState::Up, 15, 2))
            .unwrap();
        let original = event(ButtonState::Down, 21, 3);
        let mut parked = Vec::new();
        park_resume_event(&mut parked, original.clone()).unwrap();
        assert_eq!(parked[0], original);
        // A bounded collector iteration still has backlog: no reconciliation
        // or admitted parked Down occurs until the actual empty observation.
        assert_eq!(parked.len(), 1);
        let mut ordered = keyboard.resume(point(20)).unwrap();
        ordered.extend(parked.drain(..));
        assert_eq!(ordered[0].meta().timestamp, point(20).timestamp);
        assert_eq!(ordered[0].meta().original_clock_point, Some(point(15)));
        assert_eq!(ordered[1], original);
        assert!(keyboard.accept(&ordered[1]).unwrap());
        let mut full = vec![original.clone(); 4096];
        assert!(park_resume_event(&mut full, original.clone()).is_err());
        assert_eq!(full.len(), 4096);
        assert_eq!(full.last(), Some(&original));
    }
    #[test]
    fn gauge_selection_uses_exact_names_and_rejects_invalid_or_duplicate_values() {
        use beatkernel_bms::BmsGaugeKind;
        use beatkernel_bms_runtime::play_policy::GaugeSelection;
        let base = args();
        assert_eq!(parse(&base).unwrap().gauge, GaugeSelection::BeatKernel);
        for (name, selection) in [
            ("beatkernel", GaugeSelection::BeatKernel),
            ("assist-easy", GaugeSelection::Bms(BmsGaugeKind::AssistEasy)),
            ("easy", GaugeSelection::Bms(BmsGaugeKind::Easy)),
            ("groove", GaugeSelection::Bms(BmsGaugeKind::Groove)),
            ("hard", GaugeSelection::Bms(BmsGaugeKind::Hard)),
            ("ex-hard", GaugeSelection::Bms(BmsGaugeKind::ExHard)),
            ("hazard", GaugeSelection::Bms(BmsGaugeKind::Hazard)),
        ] {
            let mut supplied = base.clone();
            supplied.extend(["--gauge".into(), name.into()]);
            assert_eq!(parse(&supplied).unwrap().gauge, selection);
            assert!(validate_args(&supplied).is_ok());
        }
        for value in ["", "Hard", " hard", "hard ", "exhard", "unknown"] {
            let mut supplied = base.clone();
            supplied.extend(["--gauge".into(), value.into()]);
            assert!(parse(&supplied).is_err(), "{value:?}");
        }
        let mut duplicate = base.clone();
        duplicate.extend([
            "--gauge".into(),
            "hard".into(),
            "--gauge".into(),
            "easy".into(),
        ]);
        assert!(parse(&duplicate).is_err());
        let mut missing = base;
        missing.push("--gauge".into());
        assert!(parse(&missing).is_err());
    }

    #[test]
    fn nondefault_gauge_competition_is_rejected_without_opening_resources() {
        for (flag, value) in [
            ("--ghost-self", "unopened.bkr"),
            ("--ghost-other", "unopened.bkr"),
            ("--mp-host", "127.0.0.1:34567"),
        ] {
            let mut supplied = args();
            supplied.extend([flag.into(), value.into()]);
            assert!(validate_args(&supplied).is_ok());
            supplied.extend(["--gauge".into(), "hard".into()]);
            if flag.starts_with("--ghost") {
                assert!(validate_args(&supplied).is_ok());
            } else {
                assert!(validate_args(&supplied).is_err());
            }
        }
    }
}
