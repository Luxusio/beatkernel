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
use beatkernel_bms_runtime::native_finish::{finish_solo, save_capture};

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
    competition: &beatkernel_bms_runtime::competition_live::CompetitionOptions,
) -> Result<()> {
    options.playback_end()?;
    let _ = competition; // Network cohorts use the common committed-start owner.
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
    let (competition_options, args) =
        beatkernel_bms_runtime::competition_live::CompetitionOptions::extract(args)?;
    if args.is_empty() || args == ["--help"] {
        println!(
            "linux_bms --chart PATH (--evdev NODE | repeated --local-input NODE | repeated --local-player ID:PATH) --alsa ENDPOINT --rate HZ --channels N --period-frames N --buffer-frames N [--seconds N] --bind channelHEX:HIDusageHEX [--bind ...]\nOptions: --record-replay PATH --replay-max-records N --replay-max-bytes N --early-ns N --late-ns N --input-offset-ns N --chart-seed DECIMAL_U64 --start-ns N --end-ns N --preroll-ns N --bgm-lookahead-ns N --advance-lag-ns N --voices N --channel-policy exact|mono-stereo\nBounds: start unsigned0..9223372036854775807ns, BGM lookahead positive i64 ns, seconds 1..3600, preroll 0..10000000000 ns, advance lag 0..1000000000 ns, voices 1..4096. Defaults: chart seed0, replay disabled, max records 1000000, max bytes 67108864, BGM lookahead3000000000ns, windows 150000000 ns, offset 0 ns, preroll 3000000000 ns, advance lag 2000000 ns, voices 256, exact channels. Optional --end-ns is unsigned, strictly after start, and supports solo or local-cohort network peers with the same section endpoint; it completes a finite prefix after native presentation and input drain, without forcing unfinished notes. Missing --seconds plays the full song through terminal judging and reported native audio presentation; --seconds is a diagnostic loop cutoff after startup. Solo exact one-node bindings; repeated --local-input assigns sequential player IDs to 2..64 devices; repeated --local-player ID:PATH preserves unique positive u32 IDs. Do not mix local forms or --evdev. Exact paths retain colons after the first ID separator. Local cohorts share lane bindings and output. Local replay paths gain .p<ID>.bkr; The winit/wgpu graphical player uses these same native options/local panels; local groups use one shared QUIC/WebTransport connection and committed native start. Native float32 ALSA, no fallback. Physical timing Unknown."
        );
        return Ok(());
    }
    let options = parse(&args)?;
    validate_finite_modes(&options, &competition_options)?;
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
    };
    use beatkernel_bms_runtime::{
        ChannelPolicy,
        native_chart::{NativeChartConfig, prepare_chart},
        native_judge::{NativeJudgeConfig, capture_limits, prepare_capture_for_source},
        playback_pause::NativePause,
        player::{self},
    };
    use beatkernel_platform::{
        audio::{
            DeviceFormat, SampleEncoding,
            presentation::discipline::{DisciplineConfig, PresentationDiscipline},
        },
        linux::{
            AlsaRequest, AlsaStatus, AlsaStream, EvdevDevice, EvdevItem, MonotonicClock,
            alsa_presentation_pair,
        },
    };
    use std::{
        collections::VecDeque,
        time::{Duration as WallDuration, Instant},
    };
    pub(super) const HOST: ClockDomainId = ClockDomainId(1);
    pub(super) const OUTPUT: ClockDomainId = ClockDomainId(2);
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
        Ok(alsa_presentation_pair(
            timing,
            output_origin(),
            stream.configuration().format.sample_rate(),
        )?)
    }
    use beatkernel_bms_runtime::native_start::{
        MAX_START_INPUT_EVENTS, NativeStartConfig, NativeStartDevice, NativeStartObservation,
        NativeStartResult, start_committed,
    };
    struct StartupDevice<'a> {
        stream: &'a mut AlsaStream,
        input: &'a mut EvdevDevice,
        clock: &'a MonotonicClock,
        before_origin: &'a mut u64,
        retained: &'a mut VecDeque<PhysicalInputEvent>,
        observed: bool,
    }
    impl NativeStartDevice for StartupDevice<'_> {
        type Evidence = ();
        fn start(&mut self) -> NativeStartResult<()> {
            Ok(self.stream.start()?)
        }
        fn service_input(&mut self, retain: bool) -> NativeStartResult<bool> {
            startup_input(
                self.input,
                self.before_origin,
                if retain {
                    Some(&mut *self.retained)
                } else {
                    None
                },
            )
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
        InputBatch, NativeGameplayConfig, NativeGameplayDevice, NativeGameplayResult,
        NativeGameplaySession, retain_input, run_gameplay,
    };
    struct GameplayDevice<'a> {
        stream: &'a mut AlsaStream,
        input: &'a mut EvdevDevice,
        clock: &'a MonotonicClock,
        retained: &'a mut VecDeque<PhysicalInputEvent>,
    }
    impl NativeGameplayDevice for GameplayDevice<'_> {
        fn observe(&mut self, discipline: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
            if let Some(pair) = observe(self.stream, false)? {
                discipline.observe_clock_pair(pair)?;
            }
            Ok(())
        }
        fn render_report(
            &mut self,
        ) -> NativeGameplayResult<Option<beatkernel::audio::RenderReport>> {
            Ok(self.stream.last_render_report())
        }
        fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
            Ok(self.clock.now()?)
        }
        fn acquire(
            &mut self,
            events: &mut VecDeque<PhysicalInputEvent>,
        ) -> NativeGameplayResult<InputBatch> {
            for _ in 0..256 {
                let next = if let Some(event) = self.retained.pop_front() {
                    EvdevItem::Event(event)
                } else {
                    self.input.read_next()?
                };
                match next {
                    EvdevItem::WouldBlock => {
                        return Ok(InputBatch {
                            backlog: false,
                            closed: false,
                        });
                    }
                    EvdevItem::Ignored => {}
                    EvdevItem::Event(event) => retain_input(events, event)?,
                    EvdevItem::Dropped | EvdevItem::Resync(_) => {
                        return Err("evdev loss/resync; cleanup and restart required".into());
                    }
                }
            }
            Ok(InputBatch {
                backlog: true,
                closed: false,
            })
        }
        fn observe_end(
            &mut self,
            end: &mut beatkernel_bms_runtime::native_end::NativeEnd,
            discipline: &PresentationDiscipline,
            report: Option<beatkernel::audio::RenderReport>,
        ) -> NativeGameplayResult<Option<beatkernel_bms_runtime::native_end::EndBoundary>> {
            Ok(end.observe(
                report,
                discipline
                    .latest_pair()
                    .ok_or("native end clock relation missing")?,
            )?)
        }
        fn seed_resume(
            &mut self,
            discipline: &mut PresentationDiscipline,
            reference: ClockPair,
        ) -> NativeGameplayResult<()> {
            discipline.observe_clock_pair(reference)?;
            Ok(())
        }
        fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
            Err("ALSA uses logical mixer scheduling".into())
        }
    }
    // The owner drains bounded batches; the audio callback never waits on input.
    pub(super) fn startup_input(
        input: &mut EvdevDevice,
        before_origin: &mut u64,
        retained: Option<&mut VecDeque<PhysicalInputEvent>>,
    ) -> Result<bool> {
        if player::cancelled() {
            return Ok(false);
        }
        let mut retained = retained;
        for _ in 0..256 {
            match input.read_next()? {
                EvdevItem::WouldBlock => break,
                EvdevItem::Ignored => {}
                EvdevItem::Event(event) => {
                    if let Some(events) = retained.as_deref_mut() {
                        if events.len() == MAX_START_INPUT_EVENTS {
                            return Err("startup input capacity exceeded; restart required".into());
                        }
                        events.push_back(event);
                    } else {
                        *before_origin = before_origin.saturating_add(1);
                    }
                }
                EvdevItem::Dropped | EvdevItem::Resync(_) => {
                    return Err("evdev loss during startup; restart required".into());
                }
            }
        }
        Ok(true)
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
        let playback_end = options.playback_end()?;
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
        let judge_config = NativeJudgeConfig {
            early: options.early,
            late: options.late,
            offset: options.offset,
            preroll: options.preroll,
            output: OUTPUT,
            end: options.end_ns.map(Timestamp::from_nanos),
        };
        let mut completion = judge_config.completion(&prepared)?;
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
            &prepared.source,
            &prepared.compiled.chart,
            &[beatkernel_bms_runtime::local_players::PlayerId(1)],
        )?;
        if beatkernel_bms_runtime::player::cancelled() {
            return Ok(());
        }
        let judge = judge_config.judge(&prepared.source, prepared.compiled.chart)?;
        let mut competition =
            beatkernel_bms_runtime::competition_live::LiveCompetition::prepare_native_section_at_with_chart_seed(
                &competition_options,
                &prepared.source,
                &judge,
                HOST,
                Timestamp::from_nanos(options.start_ns),
                options.chart_seed,
                options.end_ns.map(Timestamp::from_nanos),
                options.preroll,
            )?;
        const SLACK: usize = beatkernel_bms_runtime::native_audio::LIVE_COMMAND_RESERVE;
        let capacity = AudioLimits::MAX_COMMANDS;
        let network_start = competition_options.network.is_some();
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
                max_render_frames: options.period as usize,
                playback_end_frame: playback_end,
                gated_start: network_start,
            },
        )?;
        let mut bgm = BgmSession(bgm);
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
        let mut startup_inputs = VecDeque::with_capacity(MAX_START_INPUT_EVENTS);
        let outcome = (|| -> Result<()> {
            capture = prepare_capture_for_source(
                &prepared.source,
                &judge,
                HOST,
                Timestamp::from_nanos(options.start_ns),
                options.chart_seed,
                capture_limits(
                    options.record_replay.is_some(),
                    options.replay_max_bytes,
                    options.replay_max_records,
                )?,
            )?;
            let (mut discipline, pair, host_origin, discipline_origin) = if network_start {
                let competition = competition
                    .as_mut()
                    .ok_or("network startup owner missing")?;
                let started = {
                    let mut device = StartupDevice {
                        stream: &mut stream,
                        input: &mut input,
                        clock: &clock,
                        before_origin: &mut before_origin,
                        retained: &mut startup_inputs,
                        observed: false,
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
                            feed_rendered(&mut bgm, report, |command| producer.try_push(command))
                        },
                    )?
                };
                let Some(started) = started else {
                    return Ok(());
                };
                let plan = started.plan;
                let pair = started.observation.timing.point()?;
                let host_origin = started.host_origin;
                let mut discipline = PresentationDiscipline::new_with_playback_origin(
                    DisciplineConfig::default(),
                    output_origin(),
                    plan.selected_output(),
                    HOST,
                    song_origin,
                )?;
                discipline.observe_clock_pair(pair)?;
                println!(
                    "native applied start={plan:?}; host={host_origin:?}; physical accuracy unmeasured"
                );
                (discipline, pair, host_origin, plan.selected_output())
            } else {
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
                (discipline, pair, host_origin, output_origin())
            };
            let transport = Transport::new(host_origin.timestamp, song_origin, Rate::NORMAL);
            println!(
                "estimated playback-zero host={host_origin:?}; actual seed={pair:?}; discipline={:?}; quality={:?}; physical latency unmeasured",
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
            if let Some(timeline) = input_sounds {
                runtime.configure_input_sounds(timeline)?;
            }
            if let Some(end) = options.end_ns {
                runtime.set_song_end(Timestamp::from_nanos(end))?;
            }
            let pump_outcome = {
                let mut device = GameplayDevice {
                    stream: &mut stream,
                    input: &mut input,
                    clock: &clock,
                    retained: &mut startup_inputs,
                };
                run_gameplay(
                    &mut device,
                    NativeGameplaySession {
                        runtime: &mut runtime,
                        bgm: &mut bgm,
                        discipline: &mut discipline,
                        pause: &mut pause,
                        end: &mut native_end,
                        completion: &mut completion,
                        capture: &mut capture,
                        competition: &mut competition,
                        delivery: &mut delivery,
                        pre_origin_inputs: &mut before_origin,
                    },
                    NativeGameplayConfig {
                        origin: host_origin,
                        stream_origin: output_origin(),
                        playback_origin: discipline_origin,
                        song_origin,
                        sample_rate: options.format.sample_rate(),
                        end_song: options.end_ns.map(Timestamp::from_nanos),
                        advance_lag: Duration::from_nanos(options.advance_lag),
                        seconds: options.seconds,
                        pause_supported,
                        logical_schedule: true,
                    },
                )
            };
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
        finish_solo(
            outcome,
            stop.map_err(Into::into),
            Ok(()),
            competition.as_mut(),
            capture,
            options.record_replay.as_deref(),
            save_capture,
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
}
