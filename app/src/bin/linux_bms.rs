//! Actual BMS/WAV assets, explicit evdev sources and exact native ALSA output.
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
    chart_seed: u64,
    gauge: beatkernel_bms_runtime::play_policy::GaugeSelection,
    timing: Option<beatkernel_bms_runtime::play_policy::TimingPresetSelection>,
    start_ns: i64,
    end_ns: Option<i64>,
    bgm_lookahead: i64,
    advance_lag: i64,
    voices: usize,
    mono_stereo: bool,
}
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
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
    let mut chart_seed = 0u64;
    let mut gauge = beatkernel_bms_runtime::play_policy::GaugeSelection::BeatKernel;
    let (mut timing_preset, mut rank_precedence) = (None, None);
    let mut start_ns = 0i64;
    let mut end_ns = None;
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
            "--gauge" => gauge = value.parse()?,
            "--timing-preset" => timing_preset = Some(value.clone()),
            "--rank-precedence" => rank_precedence = Some(value.clone()),
            "--chart-seed" => {
                chart_seed = beatkernel_bms_runtime::settings::parse_chart_seed(value)?;
            }
            "--start-ns" => {
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err("start-ns must be unsigned decimal nanoseconds".into());
                }
                start_ns = value.parse::<i64>()?;
            }
            "--preroll-ns" => preroll = value.parse()?,
            "--end-ns" => {
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err("end-ns must be unsigned decimal nanoseconds".into());
                }
                end_ns = Some(value.parse::<i64>()?);
            }
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
    if end_ns.is_some_and(|end| end <= start_ns) {
        return Err("end-ns must follow start-ns".into());
    }
    let evdev = evdev
        .or_else(|| local_inputs.first().cloned())
        .ok_or("explicit --evdev or multiple local assignments required")?;
    let timing = beatkernel_bms_runtime::settings::parse_timing_selection(
        timing_preset.as_deref(),
        rank_precedence.as_deref(),
    )?;
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
        chart_seed,
        gauge,
        timing,
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
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
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
use beatkernel_bms_runtime::native_audio::feed_rendered;

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
use beatkernel_bms_runtime::native_finish::{finish_solo_with_result_and_score, save_capture};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    run_args(&args)
}

/// Validate settings through the same parsers as play, without opening any resources.
#[allow(dead_code)] // Standalone native binaries have no settings screen.
pub(crate) fn validate_args(args: &[String]) -> Result<()> {
    let (competition, native) =
        beatkernel_bms_runtime::competition_live::CompetitionOptions::extract(args)?;
    validate_finite_modes(&parse(&native)?, &competition)
}

fn validate_finite_modes(
    options: &Options,
    _competition: &beatkernel_bms_runtime::competition_live::CompetitionOptions,
) -> Result<()> {
    options.playback_end()?;
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
#[cfg(test)]
fn finite_session_done(
    end_ns: Option<i64>,
    presented: Option<ClockPoint>,
    watermark: ClockPoint,
    song: Timestamp,
    backlog: bool,
    resuming: bool,
) -> bool {
    end_ns.is_some_and(|end| song.as_nanos() >= end)
        && presented.is_some_and(|boundary| {
            boundary.domain == watermark.domain && watermark.timestamp >= boundary.timestamp
        })
        && !backlog
        && !resuming
}

pub(crate) fn run_args(args: &[String]) -> Result<()> {
    let invocation = args.to_vec();
    let (competition_options, args) =
        beatkernel_bms_runtime::competition_live::CompetitionOptions::extract(args)?;
    if args.is_empty() || args == ["--help"] {
        println!(
            "linux_bms --chart PATH (--evdev NODE | repeated --local-input NODE | repeated --local-player ID:PATH) --alsa ENDPOINT --rate HZ --channels N --period-frames N --buffer-frames N [--seconds N] --bind channelHEX:HIDusageHEX [--bind ...]\nOptions: --record-replay PATH --replay-max-records N --replay-max-bytes N --early-ns N --late-ns N --input-offset-ns N --chart-seed DECIMAL_U64 --gauge beatkernel|assist-easy|easy|groove|hard|ex-hard|hazard --timing-preset PRESET_ID --rank-precedence rank-first|defexrank-first --start-ns N --end-ns N --preroll-ns N --bgm-lookahead-ns N --advance-lag-ns N --voices N --channel-policy exact|mono-stereo\nTiming preset: optional beatoraja-sevenkeys/8320241d8481e0826c703878c3eba01cd81ca3e4/v1 requires explicit rank precedence and original RANK or supported positive integer DEFEXRANK; gauges remain independently selected. Uses numerical hit windows with BeatKernel Hold v1, not full source LN/CN/HCN semantics. Gauge timing without preset: existing early/late window gives one PGREAT hit class and POOR misses with input offset; full LR2 judgment windows are not provided. Saved ghosts must match the chosen policy. Multiplayer peers must match the chosen gauge and judgment policy.\nBounds: start unsigned0..9223372036854775807ns, BGM lookahead positive i64 ns, seconds 1..3600, preroll 0..10000000000 ns, advance lag 0..1000000000 ns, voices 1..4096. Defaults: gauge beatkernel, chart seed0, replay disabled, max records 1000000, max bytes 67108864, BGM lookahead3000000000ns, windows 150000000 ns, offset 0 ns, preroll 3000000000 ns, advance lag 2000000 ns, voices 256, exact channels. Optional --end-ns is unsigned, strictly after start, and supports solo or local-cohort network peers with the same section endpoint; it completes a finite prefix after native presentation and input drain, without forcing unfinished notes. Missing --seconds plays the full song through terminal judging and reported native audio presentation; --seconds is a diagnostic loop cutoff after startup. Solo exact one-node bindings; repeated --local-input assigns sequential player IDs to 2..64 devices; repeated --local-player ID:PATH preserves unique positive u32 IDs. Do not mix local forms or --evdev. Exact paths retain colons after the first ID separator. Local cohorts share lane bindings and output. Local replay paths gain .p<ID>.bkr; The winit/wgpu graphical player uses these same native options/local panels; local groups use one shared QUIC/WebTransport connection and committed native start. Native float32 ALSA, no fallback. Physical timing Unknown."
        );
        return Ok(());
    }
    let options = parse(&args)?;
    validate_finite_modes(&options, &competition_options)?;
    #[cfg(target_os = "linux")]
    {
        if options.local_inputs.is_empty() {
            native::run(
                options,
                competition_options,
                beatkernel_bms_runtime::player::native_launch(&invocation)?,
            )
        } else {
            local_native::run(
                options,
                competition_options,
                beatkernel_bms_runtime::player::native_launch(&invocation)?,
            )
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (options, competition_options);
        Err("linux_bms native playback requires Linux".into())
    }
}

#[cfg(target_os = "linux")]
#[path = "linux_bms/input.rs"]
mod collected_input;
#[cfg(target_os = "linux")]
#[path = "linux_bms/local.rs"]
mod local_native;

#[cfg(target_os = "linux")]
mod native {
    use super::*;
    use beatkernel::{
        audio::PcmLimits,
        input::{
            Binding, BindingMap, DeviceId, DeviceSelector, GameControlId, PhysicalControlId,
            PhysicalInputEvent,
        },
        time::{ClockDomainId, Duration},
        transport::{Rate, Transport},
    };
    use beatkernel_bms_runtime::local_runtime::SoloRuntime as Runtime;
    use beatkernel_bms_runtime::native_audio::{
        NativeAudioConfig, PreparedNativeAudio, prepare_audio, prepare_input_sounds,
        prepare_mine_sounds,
    };
    use beatkernel_bms_runtime::{
        ChannelPolicy,
        native_chart::{NativeChartConfig, prepare_chart_with_policy},
        native_judge::{NativeJudgeConfig, capture_limits, prepare_section_capture_for_policy},
        playback_pause::NativePause,
        player::{self},
    };
    use beatkernel_platform::{
        audio::{
            DeviceFormat, SampleEncoding,
            presentation::discipline::{DisciplineConfig, PresentationDiscipline},
        },
        linux::{
            AlsaRequest, AlsaStatus, AlsaStream, MonotonicClock, alsa_presentation_pair_with_basis,
        },
    };
    use std::{collections::VecDeque, time::Duration as WallDuration};
    pub(super) const HOST: ClockDomainId = ClockDomainId(1);
    pub(super) const OUTPUT: ClockDomainId = ClockDomainId(2);
    pub(super) const LOGICAL: ClockDomainId = ClockDomainId(3);
    const DEVICE: DeviceId = DeviceId(1);

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
        Ok(alsa_presentation_pair_with_basis(
            timing,
            stream.frame_basis(),
        )?)
    }
    use beatkernel_bms_runtime::native_start::{
        MAX_START_INPUT_EVENTS, NativeStartConfig, NativeStartDevice, NativeStartObservation,
        NativeStartResult, NativeTargetStartDevice, start_target_committed,
    };
    struct StartupDevice<'a> {
        stream: &'a mut AlsaStream,
        input: &'a mut beatkernel_bms_runtime::native_input::NativeInputCollector,
        clock: &'a MonotonicClock,
        before_origin: &'a mut u64,
        retained: &'a mut NativeCollectedInput,
        observed: bool,
    }
    impl NativeStartDevice for StartupDevice<'_> {
        type Evidence = ();
        fn start(&mut self) -> NativeStartResult<()> {
            Ok(self.stream.start()?)
        }
        fn service_input(&mut self, retain: bool) -> NativeStartResult<bool> {
            startup_input(self.input, self.before_origin, self.retained, retain)
        }
        fn observe(&mut self) -> NativeStartResult<Option<NativeStartObservation<()>>> {
            let pair = observe(self.stream, !self.observed)?;
            self.observed |= pair.is_some();
            Ok(pair.map(|pair| NativeStartObservation {
                timing: pair.into(),
                evidence: (),
            }))
        }
        fn render_report(&mut self) -> NativeStartResult<Option<beatkernel::audio::RenderReport>> {
            Ok(self.stream.last_render_report())
        }
        fn buffer_frames(&self) -> NativeStartResult<u32> {
            Ok(self.stream.configuration().buffer_frames)
        }
        fn host_now(&self) -> NativeStartResult<ClockPoint> {
            Ok(self.clock.now()?)
        }
    }
    use beatkernel_bms_runtime::native_gameplay::{
        AudioGameplayConfig, GameplaySession, InputBatch, NativeAudioGameplaySession,
        NativeCollectedInput, NativeGameplayConfig, NativeGameplayDevice, NativeGameplayResult,
        run_gameplay_audio_with_policy_and_result_and_score,
    };
    use beatkernel_bms_runtime::{
        gameplay_output_owner::GameplayOutputOwner, native_alsa_replacement::AlsaReplacementBackend,
    };
    type OwnedOutput = GameplayOutputOwner<
        beatkernel_bms_runtime::gameplay::output::adapters::remix::RemixedOutputBackend<
            AlsaReplacementBackend,
        >,
        beatkernel_platform::audio::NativeOutputState,
    >;
    use beatkernel_bms_runtime::native_alsa_output_ui::{
        ConvertedAlsaOutputOwner, ConvertedAlsaOutputUi,
    };
    use beatkernel_platform::linux::ConvertedAlsaStream;

    pub(super) struct TargetStartupDevice<'a> {
        pub stream: &'a mut ConvertedAlsaStream,
        pub epoch: u64,
        pub input: &'a mut beatkernel_bms_runtime::native_input::NativeInputCollector,
        pub clock: &'a MonotonicClock,
        pub before_origin: &'a mut u64,
        pub retained: &'a mut NativeCollectedInput,
    }
    impl NativeStartDevice for TargetStartupDevice<'_> {
        type Evidence = ();
        fn start(&mut self) -> NativeStartResult<()> {
            Ok(self.stream.start()?)
        }
        fn service_input(&mut self, retain: bool) -> NativeStartResult<bool> {
            startup_input(self.input, self.before_origin, self.retained, retain)
        }
        fn observe(&mut self) -> NativeStartResult<Option<NativeStartObservation<()>>> {
            match self.stream.snapshot().status {
                AlsaStatus::Ready => return Ok(None),
                AlsaStatus::Running => {}
                state => {
                    return Err(format!("target ALSA terminal/unexpected state: {state:?}").into());
                }
            }
            let Some(timing) = self.stream.timing_snapshot() else {
                return Ok(None);
            };
            Ok(
                beatkernel_platform::linux::alsa_presentation_pair_with_target_basis(
                    timing,
                    self.stream.frame_basis(),
                )?
                .map(|pair| NativeStartObservation {
                    timing: pair.into(),
                    evidence: (),
                }),
            )
        }
        fn render_report(&mut self) -> NativeStartResult<Option<beatkernel::audio::RenderReport>> {
            Ok(self.stream.last_real_source_report())
        }
        fn buffer_frames(&self) -> NativeStartResult<u32> {
            Ok(self.stream.configuration().buffer_frames)
        }
        fn host_now(&self) -> NativeStartResult<ClockPoint> {
            Ok(self.clock.now()?)
        }
    }
    impl NativeTargetStartDevice for TargetStartupDevice<'_> {
        fn target_identity(&self) -> NativeStartResult<(u64, beatkernel::audio::TargetFrameBasis)> {
            Ok((self.epoch, self.stream.frame_basis()))
        }
        fn target_output_telemetry(
            &mut self,
        ) -> NativeStartResult<
            Option<beatkernel_bms_runtime::gameplay::output::ports::TargetOutputTelemetry>,
        > {
            Ok(self
                .stream
                .output_telemetry()
                .map(|(source, converted, facts)| {
                    beatkernel_bms_runtime::gameplay::output::ports::TargetOutputTelemetry {
                        source,
                        converted,
                        facts,
                    }
                }))
        }
    }

    /// Cold target preparation preserves the immutable source Mixer and command clock.
    pub(super) fn open_target_output(
        request: AlsaRequest,
        mixer: beatkernel::audio::Mixer,
    ) -> Result<ConvertedAlsaOutputOwner> {
        use beatkernel_bms_runtime::native_alsa_replacement::{
            ConvertedAlsaReplacementBackend, ConvertedAlsaReplacementRequest,
        };
        let matrix = beatkernel::audio::ChannelMatrix::default_mix(
            mixer.config().format().channels(),
            request.format.channels(),
        )?;
        let state = beatkernel_platform::audio::ConvertedNativeOutputState::new(
            mixer,
            request.format,
            matrix,
            beatkernel::audio::ResampleQuality::Linear,
            request.period_frames as usize,
        )
        .map_err(|failure| failure.into_parts().0)?;
        Ok(ConvertedAlsaOutputOwner::open_initial(
            ConvertedAlsaReplacementBackend,
            ConvertedAlsaReplacementRequest {
                native: request,
                matrix: None,
            },
            state,
            0,
        )
        .map_err(|failure| failure.into_parts().0)?)
    }
    pub(super) fn observe_target_output(
        output: &mut ConvertedAlsaOutputOwner,
        presentation: &mut beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
        end: Option<&mut beatkernel_bms_runtime::native_end::NativeEnd>,
    ) -> Result<bool> {
        output.observe_target_native(presentation)?;
        if let (Some(end), Some(record)) = (end, presentation.latest_record()) {
            let initial = output
                .current()
                .ok_or("startup target ALSA output missing")?;
            let mut next = end.clone();
            next.prime_target_clock(
                initial.epoch(),
                initial.stream().frame_basis(),
                record.pair(),
            )?;
            if let Some(facts) = output.target_boundary_facts() {
                next.prime_target(
                    initial.epoch(),
                    initial.stream().frame_basis(),
                    facts,
                    output.render_report(),
                    record.pair(),
                )?;
                *end = next;
                return Ok(true);
            }
            *end = next;
            return Ok(false);
        }
        Ok(presentation.latest_record().is_some())
    }
    enum SoloOutput {
        Legacy(
            OwnedOutput,
            beatkernel_bms_runtime::native_alsa_output_ui::NativeAlsaOutputUi,
        ),
        Target(ConvertedAlsaOutputOwner, ConvertedAlsaOutputUi),
    }
    impl SoloOutput {
        fn legacy_mut(&mut self) -> Result<&mut OwnedOutput> {
            match self {
                Self::Legacy(owner, _) => Ok(owner),
                Self::Target(..) => {
                    Err("committed startup requires the legacy source stream".into())
                }
            }
        }
        fn target(&self) -> bool {
            matches!(self, Self::Target(..))
        }
        fn configuration(&self) -> Result<&beatkernel_platform::linux::AlsaAppliedConfig> {
            match self {
                Self::Legacy(owner, _) => Ok(owner
                    .current()
                    .ok_or("ALSA output missing")?
                    .stream()
                    .configuration()),
                Self::Target(owner, _) => Ok(owner
                    .current()
                    .ok_or("target ALSA output missing")?
                    .stream()
                    .configuration()),
            }
        }
        fn start(&mut self) -> Result<()> {
            match self {
                Self::Legacy(owner, _) => owner
                    .current_mut()
                    .ok_or("ALSA output missing")?
                    .stream_mut()
                    .start()?,
                Self::Target(owner, _) => owner
                    .current_mut()
                    .ok_or("target ALSA output missing")?
                    .stream_mut()
                    .start()?,
            };
            Ok(())
        }
        fn stop(&mut self) -> Result<()> {
            match self {
                Self::Legacy(owner, _) => owner.stop()?,
                Self::Target(owner, _) => owner.stop()?,
            };
            Ok(())
        }
        fn render_report(&self) -> Option<beatkernel::audio::RenderReport> {
            match self {
                Self::Legacy(owner, _) => owner
                    .current()
                    .and_then(|o| o.stream().last_render_report())
                    .or(owner.render_report()),
                Self::Target(owner, _) => owner
                    .current()
                    .and_then(|o| o.stream().last_real_source_report())
                    .or(owner.render_report()),
            }
        }
        fn timing_snapshot(&self) -> Option<beatkernel_platform::linux::AlsaTimingSnapshot> {
            match self {
                Self::Legacy(owner, _) => {
                    owner.current().and_then(|o| o.stream().timing_snapshot())
                }
                Self::Target(owner, _) => {
                    owner.current().and_then(|o| o.stream().timing_snapshot())
                }
            }
        }
        fn snapshot(&self) -> Option<beatkernel_platform::linux::AlsaSnapshot> {
            match self {
                Self::Legacy(owner, _) => owner.current().map(|o| o.stream().snapshot()),
                Self::Target(owner, _) => owner.current().map(|o| o.stream().snapshot()),
            }
        }
        fn pending(&self) -> bool {
            match self {
                Self::Legacy(owner, ui) => owner.replacement_pending() || ui.pending(),
                Self::Target(owner, ui) => owner.replacement_pending() || ui.pending(),
            }
        }
        fn suspended(&self) -> bool {
            match self {
                Self::Legacy(owner, _) => owner.output_clock_suspended(),
                Self::Target(owner, _) => owner.output_clock_suspended(),
            }
        }
        fn observe_audio(
            &mut self,
            presentation: &mut beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
        ) -> Result<()> {
            match self {
                Self::Legacy(owner, _) => owner.observe_native(presentation)?,
                Self::Target(owner, _) => owner.observe_target_native(presentation)?,
            };
            Ok(())
        }
        fn audio_pause(
            &mut self,
            presentation: &beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
            now: ClockPoint,
        ) -> Result<beatkernel_bms_runtime::live_pause::LivePauseObservation> {
            Ok(match self {
                Self::Legacy(owner, _) => owner.audio_pause_observation(presentation, now)?,
                Self::Target(owner, _) => {
                    owner.audio_pause_observation_target(presentation, now)?
                }
            })
        }
        fn audio_end(
            &mut self,
            end: &mut beatkernel_bms_runtime::native_end::NativeEnd,
            presentation: &beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
        ) -> Result<Option<beatkernel_bms_runtime::native_end::EndBoundary>> {
            match self {
                Self::Legacy(owner, _) => owner.observe_audio_end(end, presentation),
                Self::Target(owner, _) => owner.observe_target_audio_end(end, presentation),
            }
        }
        fn service_audio(
            &mut self,
            context: beatkernel_bms_runtime::gameplay_presentation::GameplayAudioOutputContext<'_>,
            now: ClockPoint,
        ) -> Result<bool> {
            match self {
                Self::Legacy(owner, ui) => ui.service_audio(owner, context, now),
                Self::Target(owner, ui) => ui.service_audio(owner, context, now),
            }
        }
    }
    struct GameplayDevice<'a> {
        output: &'a mut SoloOutput,
        input: &'a mut beatkernel_bms_runtime::native_input::NativeInputCollector,
        clock: &'a MonotonicClock,
        retained: &'a mut NativeCollectedInput,
        startup_end: Option<&'a mut beatkernel_bms_runtime::native_end::NativeEnd>,
        startup_primed: bool,
    }
    impl NativeGameplayDevice for GameplayDevice<'_> {
        fn observe_audio(
            &mut self,
            presentation: &mut beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
        ) -> NativeGameplayResult<()> {
            self.output.observe_audio(presentation)
        }
        fn audio_pause_observation(
            &mut self,
            presentation: &beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
            now: ClockPoint,
        ) -> NativeGameplayResult<beatkernel_bms_runtime::live_pause::LivePauseObservation>
        {
            Ok(self.output.audio_pause(presentation, now)?)
        }
        fn observe_audio_end(
            &mut self,
            end: &mut beatkernel_bms_runtime::native_end::NativeEnd,
            presentation: &beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
            _: Option<beatkernel::audio::RenderReport>,
        ) -> NativeGameplayResult<Option<beatkernel_bms_runtime::native_end::EndBoundary>> {
            self.output.audio_end(end, presentation)
        }
        fn publish_paused_audio_output(
            &mut self,
            context: beatkernel_bms_runtime::gameplay_presentation::GameplayAudioOutputContext<'_>,
            now: ClockPoint,
        ) -> NativeGameplayResult<bool> {
            self.output.service_audio(context, now)
        }

        fn observe(&mut self, discipline: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
            Ok(self.output.legacy_mut()?.observe(discipline)?)
        }
        fn output_clock_suspended(&self) -> bool {
            self.output.suspended()
        }
        fn output_replacement_pending(&self) -> bool {
            self.output.pending()
        }
        fn publish_paused_output(
            &mut self,
            context: beatkernel_bms_runtime::gameplay_presentation::GameplayOutputContext<
                '_,
                PresentationDiscipline,
            >,
        ) -> NativeGameplayResult<bool> {
            match self.output {
                SoloOutput::Legacy(owner, ui) => ui.service(owner, context, self.clock.now()?),
                SoloOutput::Target(..) => {
                    Err("target output requires typed audio publication".into())
                }
            }
        }
        fn pause_observation(
            &mut self,
            pair: ClockPair,
        ) -> NativeGameplayResult<beatkernel_bms_runtime::live_pause::LivePauseObservation>
        {
            Ok(self
                .output
                .legacy_mut()?
                .pause_observation(pair, pair.target)?)
        }
        fn render_report(
            &mut self,
        ) -> NativeGameplayResult<Option<beatkernel::audio::RenderReport>> {
            Ok(self.output.render_report())
        }
        fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
            Ok(self.clock.now()?)
        }
        fn acquire(
            &mut self,
            events: &mut VecDeque<PhysicalInputEvent>,
        ) -> NativeGameplayResult<InputBatch> {
            self.retained.acquire(self.input, events, 256)
        }
        fn observe_end(
            &mut self,
            end: &mut beatkernel_bms_runtime::native_end::NativeEnd,
            discipline: &PresentationDiscipline,
            report: Option<beatkernel::audio::RenderReport>,
        ) -> NativeGameplayResult<Option<beatkernel_bms_runtime::native_end::EndBoundary>> {
            let _ = report;
            self.output.legacy_mut()?.observe_end(end, discipline)
        }
        fn seed_resume(
            &mut self,
            discipline: &mut PresentationDiscipline,
            reference: ClockPair,
        ) -> NativeGameplayResult<()> {
            let _ = reference;
            Ok(self.output.legacy_mut()?.seed_resume(discipline)?)
        }
        fn set_audio_held(&mut self, held: bool) -> NativeGameplayResult<()> {
            if let SoloOutput::Target(owner, _) = self.output {
                owner.set_target_held(held)?;
            }
            Ok(())
        }
        fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
            Err("ALSA uses logical mixer scheduling".into())
        }
    }
    pub(super) fn startup_input(
        input: &mut beatkernel_bms_runtime::native_input::NativeInputCollector,
        before_origin: &mut u64,
        retained: &mut NativeCollectedInput,
        retain: bool,
    ) -> Result<bool> {
        if player::cancelled() {
            return Ok(false);
        }
        retained.service_start(input, retain, before_origin, 256)
    }
    /// Declared finite association permission and availability for the negotiated grid.
    pub(super) fn audio_timing_config(
        stream: &AlsaStream,
    ) -> Result<(
        beatkernel_bms_runtime::audio_authority::AudioAuthorityConfig,
        Duration,
    )> {
        timing_config_for_applied(stream.configuration())
    }
    pub(super) fn target_audio_timing_config(
        stream: &ConvertedAlsaStream,
    ) -> Result<(
        beatkernel_bms_runtime::audio_authority::AudioAuthorityConfig,
        Duration,
    )> {
        timing_config_for_applied(stream.configuration())
    }
    fn timing_config_for_applied(
        applied: &beatkernel_platform::linux::AlsaAppliedConfig,
    ) -> Result<(
        beatkernel_bms_runtime::audio_authority::AudioAuthorityConfig,
        Duration,
    )> {
        let rate = i128::from(applied.format.sample_rate());
        let buffer_ns = (i128::from(applied.buffer_frames) * 1_000_000_000 + rate - 1) / rate;
        let timeout = Duration::from_nanos(i64::try_from(buffer_ns * 4 + 2_000_000_000)?);
        Ok((
            beatkernel_bms_runtime::audio_authority::AudioAuthorityConfig {
                max_observation_age: timeout,
                input_extrapolation: beatkernel::time::ExtrapolationPolicy::Bounded {
                    before: timeout,
                    after: Duration::ZERO,
                },
                ..Default::default()
            },
            timeout,
        ))
    }
    pub(super) fn startup_render(
        stream: &AlsaStream,
    ) -> Result<Option<beatkernel::audio::RenderReport>> {
        match stream.snapshot().status {
            AlsaStatus::Ready | AlsaStatus::Running => Ok(stream.last_render_report()),
            state => Err(format!("ALSA terminal/unexpected startup state: {state:?}").into()),
        }
    }
    impl beatkernel_bms_runtime::native_audio_startup::NativeAudioSeedPort for GameplayDevice<'_> {
        fn service_input(&mut self) -> NativeGameplayResult<bool> {
            let mut ignored = 0;
            startup_input(self.input, &mut ignored, self.retained, true)
        }
        fn observe_audio(
            &mut self,
            presentation: &mut beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
        ) -> NativeGameplayResult<()> {
            match self.output {
                SoloOutput::Legacy(owner, _) => {
                    owner.observe_native(presentation)?;
                    if !self.startup_primed {
                        if let Some(record) = presentation.latest_record() {
                            if let Some(end) = self.startup_end.as_deref_mut() {
                                end.prime(
                                    startup_render(
                                        owner
                                            .current()
                                            .ok_or("startup ALSA output missing")?
                                            .stream(),
                                    )?,
                                    record.pair(),
                                )?;
                            }
                            self.startup_primed = true;
                        }
                    }
                }
                SoloOutput::Target(owner, _) => {
                    let primed = observe_target_output(
                        owner,
                        presentation,
                        if self.startup_primed {
                            None
                        } else {
                            self.startup_end.as_deref_mut()
                        },
                    )?;
                    self.startup_primed |= primed;
                }
            }
            Ok(())
        }
        fn render_report(
            &mut self,
        ) -> NativeGameplayResult<Option<beatkernel::audio::RenderReport>> {
            Ok(self.output.render_report())
        }
        fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
            Ok(self.clock.now()?)
        }
        fn wait(&mut self, duration: WallDuration) -> NativeGameplayResult<()> {
            std::thread::sleep(duration);
            Ok(())
        }
    }
    pub(super) fn run(
        options: Options,
        competition_options: beatkernel_bms_runtime::competition_live::CompetitionOptions,
        launch: beatkernel_bms_runtime::session_launch::SessionLaunch,
    ) -> Result<()> {
        let clock = MonotonicClock::new(HOST);
        // Declared before device owners so every exit reports after their cleanup.
        let mut delivery = DeliverySession(beatkernel::telemetry::InputDeliveryTelemetry::new(
            4096, HOST,
        )?);
        let song_origin = options.song_origin()?;
        let retained = competition_options.network.is_none();
        let playback_end = if retained {
            None
        } else {
            options.playback_end()?
        };
        // Validate the same endpoint grids as local playback before asset/device owners.
        let mut pause = NativePause::new(output_origin(), HOST, options.format.sample_rate())?;
        if let Some(end) = playback_end {
            pause = pause.with_playback_end_frame(end)?;
        }
        let mut native_end = playback_end
            .map(|end| {
                beatkernel_bms_runtime::native_end::NativeEnd::new(
                    output_origin(),
                    HOST,
                    options.format.sample_rate(),
                    end,
                )
            })
            .transpose()?;
        let pause_supported = competition_options.network.is_none();
        let judge_config = NativeJudgeConfig {
            early: options.early,
            late: options.late,
            offset: options.offset,
            preroll: options.preroll,
            output: OUTPUT,
            end: options.end_ns.map(Timestamp::from_nanos),
        };
        let chart_config = NativeChartConfig {
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
        };
        let (prepared, initial_source, section, policy) = if retained {
            let prepared =
                beatkernel_bms_runtime::native_chart::prepare_retained_chart_with_policy(
                    chart_config,
                    &judge_config,
                    options.gauge,
                    options.timing,
                )?;
            (
                prepared.original,
                Some(prepared.initial_source),
                prepared.section,
                prepared.policy,
            )
        } else {
            let (prepared, section, policy) = prepare_chart_with_policy(
                chart_config,
                &judge_config,
                options.gauge,
                options.timing,
            )?;
            (prepared, None, section, policy)
        };
        let max_target = if retained {
            Some(
                beatkernel_bms_runtime::completion::SongCompletion::prepare(
                    &prepared,
                    policy.completion_late().as_nanos(),
                    policy.judge().input_offset().as_nanos(),
                    options.preroll,
                    OUTPUT,
                )?
                .song_extent()
                .max(
                    options
                        .end_ns
                        .map(Timestamp::from_nanos)
                        .unwrap_or(Timestamp::ZERO),
                ),
            )
        } else {
            None
        };
        let original_source = prepared.source.clone();
        // Only judgment identities are selected. Original PCM/sounds/cues stay intact.
        let initial_source = initial_source.unwrap_or_else(|| prepared.source.clone());
        let initial_chart = initial_source.source.compile()?;
        println!("prepared practice section={section:?}");
        if let Some(timing) = policy.timing() {
            println!(
                "applied timing preset={} precedence={:?} difficulty={:?} semantics={}",
                timing.selection().preset.id(),
                timing.selection().precedence,
                timing.profiles().difficulty(),
                timing.interaction_semantics()
            );
        }
        let input_sounds = prepare_input_sounds(&prepared)?;
        let hazard_sounds = prepare_mine_sounds(&prepared, input_sounds.as_ref())?;
        let mut gauge = beatkernel_bms_runtime::gauge::BmsGauge::new(policy.gauge().try_copy()?);
        let mut completion = judge_config.completion_with_policy(&prepared, &policy)?;
        for warning in &prepared.source.warnings {
            eprintln!("BMS warning line{}: {}", warning.line, warning.message);
        }
        let bindings =
            BindingMap::from_bindings(options.bindings.iter().map(|(&channel, &key)| Binding {
                device: DeviceSelector::Exact(DEVICE),
                physical: PhysicalControlId::keyboard(key),
                game_control: GameControlId(u32::from(channel)),
            }))?;
        beatkernel_bms_runtime::player::publish_native_chart(
            &options.chart,
            &initial_source,
            &initial_chart,
            &[beatkernel_bms_runtime::local_players::PlayerId(1)],
        )?;
        if beatkernel_bms_runtime::player::cancelled() {
            return Ok(());
        }
        let judge = judge_config.judge_with_policy(&initial_source, initial_chart, &policy)?;
        if retained {
            completion = completion
                .as_ref()
                .map(|template| template.prepare_practice(&initial_source, &judge))
                .transpose()?;
        }
        let mut competition =
            beatkernel_bms_runtime::competition_live::LiveCompetition::prepare_native_section_with_policy(
                &competition_options,
                &initial_source,
                &judge,
                &policy,
                LOGICAL,
                Timestamp::from_nanos(options.start_ns),
                options.chart_seed,
                options.end_ns.map(Timestamp::from_nanos),
                options.preroll,
            )?;
        let mut capture = prepare_section_capture_for_policy(
            &initial_source,
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
        const SLACK: usize = beatkernel_bms_runtime::native_audio::LIVE_COMMAND_RESERVE;
        let capacity = AudioLimits::MAX_COMMANDS;
        let network_start = competition_options.network.is_some();
        let audio_config = NativeAudioConfig {
            output_origin: output_origin(),
            start: Timestamp::from_nanos(options.start_ns),
            preroll: Duration::from_nanos(options.preroll),
            lookahead: Duration::from_nanos(options.bgm_lookahead),
            voices: options.voices,
            max_render_frames: AudioLimits::MAX_RENDER_FRAMES,
            playback_end_frame: playback_end,
            gated_start: network_start,
        };
        let initial_region = retained
            .then(|| {
                beatkernel::audio::PracticeRegion::new(
                    song_origin,
                    options
                        .end_ns
                        .map(Timestamp::from_nanos)
                        .unwrap_or(Timestamp::from_nanos(i64::MAX)),
                    false,
                )
            })
            .transpose()?;
        let (audio, mut practice) = if let Some(region) = initial_region {
            let policy_copy = match options.timing {
                Some(timing) => {
                    beatkernel_bms_runtime::play_policy::ResolvedPlayPolicy::with_timing(
                        &original_source,
                        options.gauge,
                        timing,
                        options.offset,
                    )?
                }
                None => judge_config.resolve_play_policy(&section.original_gauge, options.gauge)?,
            };
            let overlap = beatkernel_bms_runtime::native_audio::required_bgm_overlap(
                &prepared.bank,
                &prepared.bgm_commands,
            )?;
            let gameplay_voices = options
                .voices
                .checked_sub(overlap)
                .ok_or("BGM overlap exceeds total voice budget")?;
            if gameplay_voices == 0
                && (!prepared.sounds.is_empty()
                    || !prepared.source.invisible.is_empty()
                    || !prepared.source.mines.is_empty())
            {
                return Err("total voice budget leaves no gameplay voice".into());
            }
            let limits = beatkernel::audio::PracticeLimits::new(
                prepared.bgm_commands.len().max(1),
                overlap,
                AudioLimits::MAX_COMMANDS,
                8,
                256,
            )?;
            let prepared_audio = beatkernel_bms_runtime::native_audio::prepare_retained_audio(
                prepared.bank,
                prepared.bgm_commands,
                NativeAudioConfig {
                    voices: gameplay_voices,
                    ..audio_config
                },
                beatkernel_bms_runtime::native_audio::NativePracticeAudioConfig { region, limits },
            )?;
            let practice =
                beatkernel_bms_runtime::practice_playback::PracticePlayback::new_with_completion(
                    prepared_audio.practice,
                    original_source,
                    vec![beatkernel_bms_runtime::practice_playback::PracticeMember {
                        player: beatkernel_bms_runtime::local_players::PlayerId(1),
                        policy: policy_copy,
                        launch,
                        chart_seed: options.chart_seed,
                        capture_limits: capture_limits(
                            options.record_replay.is_some(),
                            options.replay_max_bytes,
                            options.replay_max_records,
                        )?,
                    }],
                    region,
                    max_target.unwrap(),
                    prepared_audio.audio.mixer.output_frame_basis(),
                    u64::from(options.buffer),
                    options.end_ns.map(Timestamp::from_nanos),
                )?;
            (prepared_audio.audio, Some(practice))
        } else {
            (
                prepare_audio(prepared.bank, prepared.bgm_commands, audio_config)?,
                None,
            )
        };
        let PreparedNativeAudio {
            mut producer,
            bgm,
            mixer,
        } = audio;
        let mut bgm = BgmSession(bgm);
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
        let owner = open_target_output(request, mixer)?;
        let initial = owner
            .current()
            .ok_or("initial target ALSA output missing")?;
        let basis = initial.stream().frame_basis();
        if !network_start {
            pause = pause.with_target_basis(initial.epoch(), basis)?;
            native_end = native_end
                .map(|end| end.with_target_basis(initial.epoch(), basis))
                .transpose()?;
        }
        let ui = ConvertedAlsaOutputUi::new(&owner, !network_start)?;
        let mut output = SoloOutput::Target(owner, ui);
        // Heavy asset/output preparation precedes acquisition so setup cannot
        // fill the bounded transport with idle completion markers.
        let (mut input, descriptors, input_counters) =
            super::collected_input::open(vec![options.evdev.clone()])?;
        println!(
            "requested/applied ALSA={:?}; evdev={:?}; exact source={:?}; bindings={:?}; windows={}/{}ns offset={}ns preroll={}ns advance_lag={}ns voices={} channel_policy={} queue/pending={} live_slack={SLACK}",
            output.configuration()?,
            descriptors[0],
            DEVICE,
            options.bindings,
            policy.judge().max_early().as_nanos(),
            policy.judge().max_late().as_nanos(),
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
        let mut startup_inputs = NativeCollectedInput::new()?;
        let mut score = beatkernel_bms_runtime::competition::ScoreSummary::default();
        let outcome =
            (|| -> Result<Option<beatkernel_bms_runtime::play_result::CompletedPlayResult>> {
                let (network_origin, playback_origin) = if network_start {
                    let competition = competition
                        .as_mut()
                        .ok_or("network startup owner missing")?;
                    let started = {
                        let SoloOutput::Target(owner, _) = &mut output else {
                            return Err("network startup requires target output".into());
                        };
                        let initial = owner
                            .current_mut()
                            .ok_or("startup target ALSA output missing")?;
                        let epoch = initial.epoch();
                        let mut device = TargetStartupDevice {
                            stream: initial.stream_mut(),
                            epoch,
                            input: &mut input,
                            clock: &clock,
                            before_origin: &mut before_origin,
                            retained: &mut startup_inputs,
                        };
                        start_target_committed(
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
                        "native applied start={:?}; host={:?}; physical accuracy unmeasured",
                        started.plan, started.host_origin
                    );
                    (Some(started.host_origin), started.plan.selected_output()?)
                } else {
                    output.start()?;
                    (None, output_origin())
                };
                let (mut presentation, startup_timeout) = match &output {
                    SoloOutput::Legacy(owner, _) => {
                        let stream = owner
                            .current()
                            .ok_or("startup ALSA output missing")?
                            .stream();
                        let (config, timeout) = audio_timing_config(stream)?;
                        (
                            beatkernel_bms_runtime::native_audio_startup::new_audio_presentation(
                                owner.current().unwrap().epoch(),
                                stream.frame_basis(),
                                HOST,
                                ClockPoint {
                                    domain: LOGICAL,
                                    timestamp: Timestamp::ZERO,
                                },
                                config,
                            )?,
                            timeout,
                        )
                    }
                    SoloOutput::Target(owner, _) => {
                        let initial = owner
                            .current()
                            .ok_or("startup target ALSA output missing")?;
                        let (config, timeout) = target_audio_timing_config(initial.stream())?;
                        (beatkernel_bms_runtime::native_audio_startup::new_target_audio_presentation(initial.epoch(), initial.stream().frame_basis(), HOST, ClockPoint { domain: LOGICAL, timestamp: Timestamp::ZERO }, config)?, timeout)
                    }
                };
                let target = output.target();
                let seeded = {
                    let mut device = GameplayDevice {
                        output: &mut output,
                        input: &mut input,
                        clock: &clock,
                        retained: &mut startup_inputs,
                        startup_end: if network_origin.is_none() {
                            native_end.as_mut()
                        } else {
                            None
                        },
                        startup_primed: false,
                    };
                    if target {
                        beatkernel_bms_runtime::native_audio_startup::prime_target_native_audio(
                            &mut device,
                            &mut presentation,
                            &mut bgm,
                            &mut producer,
                            startup_timeout,
                        )?
                    } else {
                        beatkernel_bms_runtime::native_audio_startup::prime_native_audio(
                            &mut device,
                            &mut presentation,
                            &mut bgm,
                            &mut producer,
                            startup_timeout,
                        )?
                    }
                };
                let Some(seeded) = seeded else {
                    return Ok(None);
                };
                let host_origin = match network_origin {
                    Some(origin) => origin,
                    None => seeded.host_for_output(playback_origin, startup_timeout)?,
                };
                let logical_origin = presentation.logical_output(playback_origin)?;
                let transport = Transport::new(logical_origin.timestamp, song_origin, Rate::NORMAL);
                let mut merger = beatkernel_bms_runtime::local_input::InputMerger::new_dynamic(
                    HOST,
                    host_origin,
                    4096,
                    MAX_START_INPUT_EVENTS,
                )?;
                println!(
                    "playback origin HOST={host_origin:?}; original native anchors={:?}; logical={logical_origin:?}; mapping accuracy unknown, physical latency unmeasured",
                    seeded.observations
                );
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
                if retained {
                    runtime.set_audio_scope(beatkernel::audio::CommandScope(1));
                }
                if let Some(timeline) = input_sounds {
                    runtime.configure_input_sounds(timeline)?;
                }
                if let Some(timeline) = hazard_sounds {
                    runtime.configure_hazard_sounds(timeline)?;
                }
                if let Some(end) = options.end_ns {
                    runtime.set_song_end(Timestamp::from_nanos(end))?;
                }
                let pump_outcome = {
                    let mut device = GameplayDevice {
                        output: &mut output,
                        input: &mut input,
                        clock: &clock,
                        retained: &mut startup_inputs,
                        startup_end: None,
                        startup_primed: false,
                    };
                    let session = NativeAudioGameplaySession {
                        session: GameplaySession {
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
                            pre_origin_inputs: &mut before_origin,
                        },
                        merger: &mut merger,
                    };
                    let config = AudioGameplayConfig {
                        gameplay: NativeGameplayConfig {
                            origin: host_origin,
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
                        section_start: Timestamp::from_nanos(options.start_ns),
                    };
                    if let Some(practice) = practice.as_mut() {
                        let mut recording = beatkernel_bms_runtime::native_gameplay::NativePracticeRecorder::new(save_capture);
                        beatkernel_bms_runtime::native_gameplay::run_gameplay_audio_with_practice_and_result_and_score(
                            &mut device, session, config, &mut score, &policy, practice, &mut recording)
                    } else {
                        run_gameplay_audio_with_policy_and_result_and_score(
                            &mut device,
                            session,
                            config,
                            &mut score,
                            &policy,
                        )
                    }
                };
                println!(
                    "runtime processing={:?} counters={:?}; pre-origin ignored={before_origin}; evdev collector active",
                    runtime.telemetry().processing(),
                    runtime.telemetry().counters(),
                );
                pump_outcome
            })();
        let timing = output.timing_snapshot();
        input.cancel();
        let stop = output.stop();
        let input_stop = input.stop_and_join();
        match output.render_report() {
            Some(report) => println!(
                "last observed Mixer render report={report:?}; execution counters distinct from queue admission/native writes; physical delivery unverified"
            ),
            None => println!(
                "last observed Mixer render report unavailable; no render observation substituted"
            ),
        }
        println!(
            "final independent ALSA counters={:?}; last separately coherent timing={timing:?}; pre-origin ignored={before_origin}; physical latency=unmeasured",
            output.snapshot(),
        );
        if let Err(error) = &stop {
            eprintln!("ALSA stop/join error: {error}");
        }
        println!("final evdev counters={:?}", input_counters.try_recv().ok());
        drop(input);
        let current_recording = practice.as_ref().and_then(|practice| {
            practice.members()[0]
                .launch
                .args()
                .chunks_exact(2)
                .find(|pair| pair[0] == "--record-replay")
                .map(|pair| PathBuf::from(&pair[1]))
        });
        let final_path = if retained {
            current_recording.as_deref()
        } else {
            options.record_replay.as_deref()
        };
        finish_solo_with_result_and_score(
            outcome,
            stop.map_err(Into::into),
            input_stop.map_err(Into::into),
            competition.as_mut(),
            capture,
            gauge.profile(),
            &score,
            final_path,
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
    fn finite_end_is_exclusive_unsigned_and_mapped_once_from_original_section() {
        let base = args();
        assert_eq!(parse(&base).unwrap().playback_end().unwrap(), None);
        let mut configured = base.clone();
        configured.extend([
            "--start-ns".into(),
            "604800000000000".into(),
            "--end-ns".into(),
            "604800001000001".into(),
            "--preroll-ns".into(),
            "0".into(),
        ]);
        let options = parse(&configured).unwrap();
        let expected =
            (u128::from(options.format.sample_rate()) * 1_000_001).div_ceil(1_000_000_000);
        assert_eq!(options.playback_end().unwrap(), Some(expected as u64));
        assert!(validate_args(&configured).is_ok());
        for value in ["", "-1", "+1", "0", "1.5", "9223372036854775808"] {
            let mut invalid = base.clone();
            invalid.extend(["--end-ns".into(), value.into()]);
            assert!(validate_args(&invalid).is_err());
        }
        let mut duplicate = base;
        duplicate.extend(["--end-ns".into(), "2".into(), "--end-ns".into(), "3".into()]);
        assert!(validate_args(&duplicate).is_err());
    }
    #[test]
    fn finite_solo_and_local_group_network_are_admitted() {
        let mut local = args();
        let index = local.iter().position(|flag| flag == "--evdev").unwrap();
        local.drain(index..index + 2);
        local.extend([
            "--local-input".into(),
            "/dev/input/event4".into(),
            "--local-input".into(),
            "/dev/input/event5".into(),
        ]);
        assert!(validate_args(&local).is_ok());
        local.extend(["--end-ns".into(), "2000000".into()]);
        assert!(validate_args(&local).is_ok());
        let mut network = args();
        network.extend(["--mp-host".into(), "127.0.0.1:39001".into()]);
        assert!(validate_args(&network).is_ok());
        network.extend(["--end-ns".into(), "2000000".into()]);
        assert!(validate_args(&network).is_ok());
        local.extend(["--mp-host".into(), "127.0.0.1:39001".into()]);
        assert!(validate_args(&local).is_ok());
    }

    #[test]
    fn finite_native_pause_resume_judging_capture_and_replay_share_the_retained_prefix() {
        use beatkernel::{
            audio::{
                Mixer, MixerConfig, PcmLimits, PcmSample, SampleBank, SampleId, VoiceId,
                command_queue,
            },
            chart::{
                Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject,
                VisualId,
            },
            input::{
                Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
                GameControlId, PhysicalControlId, PhysicalInputEvent,
            },
            interaction::InstantEvaluator,
            judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow, Rule},
            replay::{ReplaySession, codec::ReplayCodecLimits},
            runtime::SoundBinding,
            time::{ClockMapper, ClockMappingQuality, Duration},
            transport::{Rate, Transport},
        };
        use beatkernel_bms_runtime::{
            local_runtime::SoloRuntime, native_end::NativeEnd, playback_pause::NativePause,
            replay_capture::LiveReplayCapture,
        };
        struct Identity;
        impl ClockMapper for Identity {
            fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
                None
            }
            fn quality(&self) -> ClockMappingQuality {
                ClockMappingQuality::Exact
            }
        }
        let judge = || {
            let mut source = SourceChart::new(1000, Bpm::new(60, 1).unwrap()).unwrap();
            for id in [1, 2] {
                source.objects.push(SourceObject {
                    id: ObjectId(id),
                    start: Beat::new(id as i64).unwrap(),
                    end: None,
                    interaction: InteractionId(1),
                    visual: VisualId(1),
                    audio: None,
                    metadata: ObjectMetadata::default(),
                });
            }
            JudgeEngine::new(
                source.compile().unwrap(),
                vec![Rule {
                    interaction: InteractionId(1),
                    control: GameControlId(1),
                    evaluator: Box::new(InstantEvaluator),
                }],
                JudgeProfile::new(
                    vec![JudgeWindow {
                        grade: JudgeGrade(1),
                        early: Duration::ZERO,
                        late: Duration::ZERO,
                    }],
                    Duration::ZERO,
                )
                .unwrap(),
            )
            .unwrap()
        };
        let output = |ns| ClockPoint {
            domain: ClockDomainId(2),
            timestamp: Timestamp::from_nanos(ns),
        };
        let pair = |ns| ClockPair {
            source: output(ns),
            target: point(ns + 100),
        };
        let format = AudioFormat::new(1000, 1).unwrap();
        let pcm_limits = PcmLimits::new(1024, 1024, 2).unwrap();
        let mut bank = SampleBank::new(format, pcm_limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.5; 8], pcm_limits).unwrap(),
        )
        .unwrap();
        let (producer, consumer) = command_queue(8).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(2),
                Timestamp::ZERO,
                AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
            )
            .with_playback_end_frame(2),
            bank,
            consumer,
        )
        .unwrap();
        let engine = judge();
        let limits = ReplayCodecLimits::new(
            65536,
            128,
            4096,
            beatkernel::input::CodecLimits::new(4096, 4096).unwrap(),
        )
        .unwrap();
        let mut capture = LiveReplayCapture::new(&engine, ClockDomainId(1), limits).unwrap();
        let mut runtime = SoloRuntime::new(
            ClockDomainId(1),
            ClockDomainId(2),
            Transport::new(Timestamp::from_nanos(100), Timestamp::ZERO, Rate::NORMAL),
            BindingMap::from_bindings([Binding {
                device: DeviceSelector::Exact(DeviceId(1)),
                physical: PhysicalControlId::keyboard(4),
                game_control: GameControlId(1),
            }])
            .unwrap(),
            engine,
            producer,
            vec![SoundBinding {
                object: ObjectId(1),
                stage: JudgeStage::Instant,
                sample: SampleId(1),
                voice: VoiceId(1),
                gain: 1.0,
            }],
            8,
        )
        .unwrap();
        runtime
            .set_song_end(Timestamp::from_nanos(2_000_000))
            .unwrap();
        let mut pause = NativePause::new(output(0), ClockDomainId(1), 1000)
            .unwrap()
            .with_playback_end_frame(2)
            .unwrap();
        let mut end = NativeEnd::new(output(0), ClockDomainId(1), 1000, 2).unwrap();
        let first = mixer.render(&mut [0.0]).unwrap();
        pause.observe(Some(first), pair(0)).unwrap();
        end.observe(Some(first), pair(0)).unwrap();
        let input = PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(DeviceId(1), point(1_000_100), 1),
            control: PhysicalControlId::keyboard(4),
            state: ButtonState::Down,
        });
        let hit = runtime
            .process_input(input, &Identity, output(1_000_000))
            .unwrap();
        assert_eq!(hit.judge_events.len(), 1);
        capture.record_report(&hit).unwrap();
        assert!(pause.request(true, pair(0)).unwrap());
        runtime.request_audio_pause(true);
        let paused = mixer.render(&mut [0.0; 3]).unwrap();
        let at = pause
            .observe(Some(paused), pair(1_000_000))
            .unwrap()
            .unwrap();
        runtime.transport_mut().pause(at.host.timestamp).unwrap();
        assert!(
            end.observe(Some(paused), pair(1_000_000))
                .unwrap()
                .is_none()
        );
        assert!(pause.request(false, pair(3_000_000)).unwrap());
        runtime.request_audio_pause(false);
        let mut pcm = [1.0; 4];
        let crossing = mixer.render(&mut pcm).unwrap();
        assert_eq!(pcm, [0.5, 0.0, 0.0, 0.0]);
        assert_eq!(crossing.playback_end_physical_frame, Some(5));
        let latest = mixer.render(&mut [0.0]).unwrap();
        let resumed = pause
            .observe(Some(latest), pair(4_000_000))
            .unwrap()
            .unwrap();
        assert!(!resumed.paused);
        runtime
            .transport_mut()
            .resume(resumed.host.timestamp)
            .unwrap();
        assert!(
            end.observe(Some(latest), pair(4_000_000))
                .unwrap()
                .is_none()
        );
        let report = runtime
            .advance_to(point(5_000_100), &Identity, output(2_000_000))
            .unwrap();
        assert!(report.song_end_reached && report.judge_events.is_empty());
        capture.record_report(&report).unwrap();
        assert!(!finite_session_done(
            Some(2_000_000),
            None,
            point(5_000_100),
            report.song_time,
            false,
            false
        ));
        let boundary = end.observe(None, pair(6_000_000)).unwrap().unwrap();
        assert_eq!(boundary.host, point(5_000_100));
        assert!(!finite_session_done(
            Some(2_000_000),
            Some(boundary.host),
            point(5_000_100),
            report.song_time,
            true,
            false
        ));
        assert!(finite_session_done(
            Some(2_000_000),
            Some(boundary.host),
            point(5_000_100),
            report.song_time,
            false,
            false
        ));
        let file = capture.into_file();
        let replay = ReplaySession::from_records(file.header, judge(), file.records).unwrap();
        assert_eq!(replay.results().len(), 1);
        assert_eq!(
            replay.engine().stable_hash().unwrap(),
            runtime.judge().stable_hash().unwrap()
        );
    }

    #[test]
    fn finite_completion_requires_presented_boundary_drained_input_and_logical_end() {
        let end = Some(2_000_000);
        let boundary = Some(point(2_001_000));
        let song = Timestamp::from_nanos(2_000_000);
        assert!(finite_session_done(
            end,
            boundary,
            point(2_001_000),
            song,
            false,
            false
        ));
        for (e, b, at, s, backlog, resuming) in [
            (None, boundary, point(2_001_000), song, false, false),
            (end, None, point(2_001_000), song, false, false),
            (end, boundary, point(2_000_999), song, false, false),
            (
                end,
                boundary,
                point(2_001_000),
                Timestamp::from_nanos(1_999_999),
                false,
                false,
            ),
            (end, boundary, point(2_001_000), song, true, false),
            (end, boundary, point(2_001_000), song, false, true),
        ] {
            assert!(!finite_session_done(e, b, at, s, backlog, resuming));
        }
        let mut wrong = point(2_001_000);
        wrong.domain = ClockDomainId(99);
        assert!(!finite_session_done(
            end, boundary, wrong, song, false, false
        ));
        // Missing physical proof cannot be replaced by a far-future timer.
        assert!(!finite_session_done(
            end,
            None,
            point(i64::MAX),
            Timestamp::MAX,
            false,
            false
        ));
    }
    #[test]
    fn chart_seed_is_shared_unsigned_u64_singleton_for_solo_and_local() {
        let base = args();
        assert_eq!(parse(&base).unwrap().chart_seed, 0);
        let mut local: Vec<String> = base
            .chunks_exact(2)
            .filter(|pair| pair[0] != "--evdev")
            .flat_map(|pair| pair.iter().cloned())
            .collect();
        local.extend([
            "--local-player".into(),
            "3:/dev/input/event3".into(),
            "--local-player".into(),
            "4294967295:/dev/input/event9".into(),
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
                            .map(|player| player.0)
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
    #[test]
    fn timing_selection_requires_exact_version_and_precedence_without_changing_gauge() {
        use beatkernel_bms::{BmsRankPrecedence, BmsTimingPreset};
        let base = args();
        assert!(parse(&base).unwrap().timing.is_none());
        let preset = BmsTimingPreset::BeatorajaSevenKeys8320241dV1;
        for (name, precedence) in [
            ("rank-first", BmsRankPrecedence::RankFirst),
            ("defexrank-first", BmsRankPrecedence::DefExRankFirst),
        ] {
            let mut supplied = base.clone();
            supplied.extend([
                "--timing-preset".into(),
                preset.id().into(),
                "--rank-precedence".into(),
                name.into(),
            ]);
            let actual = parse(&supplied).unwrap();
            assert_eq!(actual.timing.unwrap().precedence, precedence);
            assert_eq!(
                actual.gauge,
                beatkernel_bms_runtime::play_policy::GaugeSelection::BeatKernel
            );
            supplied.extend(["--gauge".into(), "hard".into()]);
            assert_eq!(
                parse(&supplied).unwrap().gauge,
                beatkernel_bms_runtime::play_policy::GaugeSelection::Bms(
                    beatkernel_bms::BmsGaugeKind::Hard
                )
            );
        }
        for added in [
            vec!["--timing-preset", preset.id()],
            vec!["--rank-precedence", "rank-first"],
            vec![
                "--timing-preset",
                "unknown",
                "--rank-precedence",
                "rank-first",
            ],
            vec![
                "--timing-preset",
                preset.id(),
                "--rank-precedence",
                "unknown",
            ],
            vec![
                "--timing-preset",
                preset.id(),
                "--rank-precedence",
                "rank-first",
                "--timing-preset",
                preset.id(),
            ],
            vec![
                "--timing-preset",
                preset.id(),
                "--rank-precedence",
                "rank-first",
                "--rank-precedence",
                "rank-first",
            ],
            vec!["--timing-preset"],
        ] {
            let mut supplied = base.clone();
            supplied.extend(added.into_iter().map(str::to_owned));
            assert!(parse(&supplied).is_err());
        }
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
    fn selected_gauge_competition_syntax_is_admitted_before_checked_policy_preparation() {
        for (flag, value) in [
            ("--ghost-self", "unopened.bkr"),
            ("--ghost-other", "unopened.bkr"),
            ("--mp-host", "127.0.0.1:34567"),
        ] {
            let mut supplied = args();
            supplied.extend([flag.into(), value.into()]);
            assert!(validate_args(&supplied).is_ok());
            supplied.extend(["--gauge".into(), "hard".into()]);
            assert!(validate_args(&supplied).is_ok());
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "linux_bms/target_solo_fixtures.rs"]
mod target_solo_fixtures;
