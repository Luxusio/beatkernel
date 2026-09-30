//! Actual BMS/WAV assets, one explicit IORegistry input and exact CoreAudio output.
#[cfg(any(target_os = "macos", test))]
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
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
struct Options {
    chart: PathBuf,
    device: u32,
    keyboard_registry: u64,
    format: AudioFormat,
    buffer: u32,
    seconds: u64,
    bindings: BTreeMap<u8, u16>,
    early: i64,
    late: i64,
    offset: i64,
    preroll: i64,
    bgm_lookahead: i64,
    advance_lag: i64,
    voices: usize,
    mono_stereo: bool,
}
fn parse(args: &[String]) -> Result<Options> {
    let (mut chart, mut device, mut keyboard_registry) = (None, None, None);
    let (mut rate, mut channels, mut buffer, mut seconds) = (None, None, None, None);
    let mut bindings = BTreeMap::new();
    let mut keys = HashSet::new();
    let mut seen = HashSet::new();
    let (mut early, mut late, mut offset, mut preroll) =
        (150_000_000i64, 150_000_000i64, 0i64, 3_000_000_000i64);
    let mut advance_lag = 2_000_000i64;
    let mut bgm_lookahead = 3_000_000_000i64;
    let mut voices = 256usize;
    let mut mono_stereo = false;
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let value = args.next().ok_or("every option requires a value")?;
        if flag != "--bind" && !seen.insert(flag.as_str()) {
            return Err(format!("duplicate option {flag}").into());
        }
        match flag.as_str() {
            "--chart" if !value.is_empty() => chart = Some(PathBuf::from(value)),
            "--device" => device = Some(value.parse::<u32>()?),
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
    let keyboard_registry = keyboard_registry.ok_or("explicit --keyboard-registry required")?;
    if device == 0
        || keyboard_registry == 0
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
    Ok(Options {
        chart: chart.ok_or("explicit --chart required")?,
        device,
        keyboard_registry,
        format: AudioFormat::new(
            rate.ok_or("explicit --rate required")?,
            channels.ok_or("explicit --channels required")?,
        )?,
        buffer,
        seconds: seconds.ok_or("explicit --seconds required")?,
        bindings,
        early,
        late,
        offset,
        preroll,
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
        println!("BGM feeder config={:?}; final admission summary={:?}; admission does not prove execution/native delivery/acoustic output", self.config(), self.report());
    }
}
#[cfg(target_os = "macos")]
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

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["--help"] {
        println!("macos_bms --chart PATH --device AUDIO_DEVICE_ID --keyboard-registry IOREGISTRY_ENTRY_ID --rate HZ --channels N --buffer-frames N --seconds N --bind channelHEX:HIDusageHEX [--bind ...]\nOptions: --early-ns N --late-ns N --input-offset-ns N --preroll-ns N --bgm-lookahead-ns N --advance-lag-ns N --voices N --channel-policy exact|mono-stereo\nBounds: BGM lookahead positive i64 ns, seconds 1..3600, preroll 0..10000000000 ns, advance lag 0..1000000000 ns, voices 1..4096. Defaults: BGM lookahead3000000000ns, windows 150000000 ns, offset 0 ns, preroll 3000000000 ns, advance lag 2000000 ns, voices 256, exact channels. Exact one-registry attachment, actual keyboard HID controls; native float32 CoreAudio, no fallback. Physical timing Unknown.");
        return Ok(());
    }
    let options = parse(&args)?;
    #[cfg(target_os = "macos")]
    {
        native::run(options)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = options;
        Err("macos_bms native playback requires macOS".into())
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use beatkernel::{
        audio::{command_queue, Mixer, MixerConfig, PcmLimits},
        input::{Binding, BindingMap, DeviceId, DeviceSelector, GameControlId, PhysicalControlId},
        judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
        runtime::{Runtime, RuntimeReport},
        time::{ClockDomainId, ClockMapper, ClockMappingQuality, Duration},
        transport::{Rate, Transport},
    };
    use beatkernel_bms_runtime::{load_prepared, ChannelPolicy};
    use beatkernel_platform::{
        audio::presentation::discipline::{
            DisciplineConfig, DisciplineUpdate, PresentationDiscipline,
        },
        macos::{
            audio::{CoreAudioRequest, CoreAudioStream},
            clock::MachClock,
            input::{HidDevice, HidInput},
            presentation::coreaudio_presentation_pair,
        },
    };
    use std::time::{Duration as WallDuration, Instant};
    const M_NATIVE: ClockDomainId = ClockDomainId(1);
    const HOST: ClockDomainId = ClockDomainId(2);
    const OUTPUT: ClockDomainId = ClockDomainId(3);
    struct ExplicitDomains;
    impl ClockMapper for ExplicitDomains {
        fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
            None
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Unknown
        }
    }
    fn output_origin() -> ClockPoint {
        ClockPoint {
            domain: OUTPUT,
            timestamp: Timestamp::ZERO,
        }
    }
    fn check_hid(input: &HidInput, selected: DeviceId, registry: u64) -> Result<()> {
        let counters = input.counters();
        if counters.queue_full != 0
            || counters.unsupported != 0
            || counters.reports_timestamp_failed != 0
        {
            return Err(format!(
                "IOHID queue/unsupported/timestamp loss; explicit restart required: {counters:?}"
            )
            .into());
        }
        if !input
            .devices()
            .iter()
            .any(|d| d.descriptor.runtime_id == selected && d.registry_entry == Some(registry))
        {
            return Err(
                "selected IORegistry attachment disconnected/reconnected; no automatic retarget"
                    .into(),
            );
        }
        Ok(())
    }
    fn select(input: &mut HidInput, registry: u64) -> Result<HidDevice> {
        let deadline = Instant::now() + WallDuration::from_secs(2);
        loop {
            let devices = input.devices();
            let mut matches = devices
                .into_iter()
                .filter(|d| d.registry_entry == Some(registry));
            if let Some(device) = matches.next() {
                if matches.next().is_some() {
                    return Err("ambiguous selected IORegistry identity".into());
                }
                if !device.descriptor.capabilities.button {
                    return Err("selected IORegistry device does not advertise buttons; choose a keyboard-capable entry".into());
                }
                return Ok(device);
            }
            if Instant::now() >= deadline {
                return Err(
                    "explicit keyboard IORegistry identity unavailable within two seconds".into(),
                );
            }
            input.poll(WallDuration::from_millis(1))?;
        }
    }
    fn observe(audio: &CoreAudioStream, clock: &MachClock) -> Result<Option<ClockPair>> {
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
    fn seed(
        audio: &CoreAudioStream,
        input: &mut HidInput,
        clock: &MachClock,
        selected: DeviceId,
        registry: u64,
        discipline: &mut PresentationDiscipline,
        bgm: &mut BgmSession,
        producer: &mut beatkernel::audio::CommandProducer,
    ) -> Result<ClockPair> {
        let deadline = Instant::now() + WallDuration::from_secs(2);
        while Instant::now() < deadline {
            input.poll(WallDuration::from_millis(1))?;
            check_hid(input, selected, registry)?;
            let pair = observe(audio, clock)?;
            feed_rendered(bgm, audio.last_render_report(), |command| {
                producer.try_push(command)
            })?;
            if let Some(pair) = pair {
                discipline.observe_clock_pair(pair)?;
                return Ok(pair);
            }
        }
        Err("no valid native CoreAudio presentation seed within two seconds".into())
    }
    fn schedule(audio: &CoreAudioStream) -> Result<ClockPoint> {
        let report = audio
            .last_render_report()
            .ok_or("successful core render boundary unavailable for keysound scheduling")?;
        let end = report
            .start_frame
            .checked_add(u64::try_from(report.frames)?)
            .ok_or("rendered frame boundary overflow")?;
        let applied = audio.configuration();
        let offset =
            (u128::from(end) * 1_000_000_000).div_ceil(u128::from(applied.format.sample_rate()));
        let at = i128::from(applied.output_origin.as_nanos())
            .checked_add(i128::try_from(offset)?)
            .ok_or("output scheduling timestamp overflow")?;
        Ok(ClockPoint {
            domain: applied.output_domain,
            timestamp: Timestamp::from_nanos(i64::try_from(at)?),
        })
    }
    fn print_report(report: RuntimeReport) -> Result<()> {
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
    pub(super) fn run(options: Options) -> Result<()> {
        let clock = MachClock::new(M_NATIVE, HOST)?;
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
        for warning in &prepared.source.warnings {
            eprintln!("BMS warning line {}: {}", warning.line, warning.message);
        }
        for note in &prepared.source.notes {
            if !options.bindings.contains_key(&note.lane.channel()) {
                return Err(
                    format!("missing --bind for BMS channel {:02X}", note.lane.channel()).into(),
                );
            }
        }
        let rules = prepared.source.rules();
        let judge = JudgeEngine::new(
            prepared.compiled.chart,
            rules,
            JudgeProfile::new(
                vec![JudgeWindow {
                    grade: JudgeGrade(1),
                    early: Duration::from_nanos(options.early),
                    late: Duration::from_nanos(options.late),
                }],
                Duration::from_nanos(options.offset),
            )?,
        )?;
        const SLACK: usize = 1024;
        let capacity = AudioLimits::MAX_COMMANDS;
        let (mut producer, consumer) = command_queue(capacity)?;
        let mut bgm = BgmSession(beatkernel_bms_runtime::bgm::BgmFeeder::new(
            prepared.bgm_commands,
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
                    options.buffer as usize,
                    capacity,
                )?,
            ),
            prepared.bank,
            consumer,
        )?;
        let mut input = HidInput::open(clock, DeviceId(1), 1024)?;
        let selected = match select(&mut input, options.keyboard_registry) {
            Ok(device) => device,
            Err(error) => {
                if let Err(close) = input.close() {
                    eprintln!("IOHID close error after selection failure: {close}");
                }
                return Err(error);
            }
        };
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
        let mut audio = match CoreAudioStream::open(request, clock, mixer) {
            Ok(audio) => audio,
            Err(error) => {
                if let Err(close) = input.close() {
                    eprintln!("IOHID close error after audio open failure: {close}");
                }
                return Err(error.into());
            }
        };
        println!("requested/applied CoreAudio={:?}; exact selected attachment={selected:?}; explicit keyboard bindings={:?}; windows={}/{}ns offset={}ns preroll={}ns advance_lag={}ns voices={} channels={} queue/pending={} live_slack={SLACK}",audio.configuration(),options.bindings,
            options.early,options.late,options.offset,options.preroll,options.advance_lag,options.voices,if options.mono_stereo{"mono-stereo"}else{"exact"},capacity);
        let mut other_devices = 0u64;
        let mut pre_origin = 0u64;
        let outcome = (|| -> Result<()> {
            check_hid(&input, selected_id, options.keyboard_registry)?;
            audio.start()?;
            let mut discipline = PresentationDiscipline::new(
                DisciplineConfig::default(),
                output_origin(),
                HOST,
                Timestamp::from_nanos(-options.preroll),
            )?;
            let pair = seed(
                &audio,
                &mut input,
                &clock,
                selected_id,
                options.keyboard_registry,
                &mut discipline,
                &mut bgm,
                &mut producer,
            )?;
            let origin = ClockPoint {
                domain: HOST,
                timestamp: estimated_origin(pair, output_origin())?,
            };
            let transport = Transport::new(
                origin.timestamp,
                Timestamp::from_nanos(-options.preroll),
                Rate::NORMAL,
            );
            println!("estimated output-zero host={origin:?}; actual seed={pair:?}; config={:?}; quality={:?}; future presentation retained, physical latency unmeasured",discipline.config(),discipline.quality());
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
            let deadline = Instant::now() + WallDuration::from_secs(options.seconds);
            let mut last_operation = origin;
            let mut last_progress = None;
            let mut waiting_logged = false;
            let pump = (|| -> Result<()> {
                while Instant::now() < deadline {
                    input.poll(WallDuration::from_millis(1))?;
                    check_hid(&input, selected_id, options.keyboard_registry)?;
                    if let Some(pair) = observe(&audio, &clock)? {
                        discipline.observe_clock_pair(pair)?;
                    }
                    feed_rendered(&mut bgm, audio.last_render_report(), |command| {
                        runtime.enqueue_audio(command)
                    })?;
                    discipline.validate_host(clock.sample()?.normalized)?;
                    let mut backlog = true;
                    for _ in 0..256 {
                        let Some(sample) = input.pop() else {
                            backlog = false;
                            break;
                        };
                        if sample.event.meta().source != selected_id {
                            other_devices = other_devices.saturating_add(1);
                            continue;
                        }
                        let host = ClockPoint {
                            domain: sample.event.meta().clock_domain,
                            timestamp: sample.event.meta().timestamp,
                        };
                        let now = clock.sample()?.normalized;
                        discipline.validate_host(now)?;
                        if !input_in_epoch(host, now, origin)? {
                            pre_origin = pre_origin.saturating_add(1);
                            if pre_origin == 1 {
                                eprintln!("ignoring pre-output-origin selected input, original native provenance retained: {:?}",sample);
                            }
                            continue;
                        }
                        validate_input_chronology(host, last_operation)?;
                        // IOHID has already normalized mach ticks once; no new clock
                        // conversion or timestamp replacement occurs in Runtime.
                        discipline.validate_host(host)?;
                        print_report(runtime.process_input(
                            sample.event,
                            &ExplicitDomains,
                            schedule(&audio)?,
                        )?)?;
                        last_operation = host;
                    }
                    let now = clock.sample()?.normalized;
                    discipline.validate_host(now)?;
                    if now.timestamp < origin.timestamp {
                        if !waiting_logged {
                            println!("waiting for future estimated output origin {origin:?}; judge operations deferred");
                            waiting_logged = true;
                        }
                        continue;
                    }
                    if let DisciplineUpdate::Applied {
                        base_rate_ppm,
                        correction_ppm,
                        applied_rate_ppm,
                        phase_error_ns,
                        limited,
                    } = discipline.update(now, runtime.transport_mut())?
                    {
                        println!("discipline measured={base_rate_ppm:+}ppm correction={correction_ppm:+}ppm applied={applied_rate_ppm:+}ppm phase={phase_error_ns}ns limited={limited} quality={:?}",discipline.quality());
                    }
                    if let Some(at) =
                        watermark(origin, last_operation, now, options.advance_lag, backlog)?
                    {
                        let report = runtime.advance_to(at, &ExplicitDomains, schedule(&audio)?)?;
                        last_operation = at;
                        let nanos = report.song_time.as_nanos();
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
                        print_report(report)?;
                    }
                }
                Ok(())
            })();
            println!("runtime processing={:?} counters={:?}; other-device ignored={other_devices}; pre-origin ignored={pre_origin}",runtime.telemetry().processing(),runtime.telemetry().counters());
            pump
        })();
        let stop = audio.stop();
        let close = input.close();
        println!("final CoreAudio native snapshot={:?}; HID counters={:?}; other-device ignored={other_devices}; pre-origin ignored={pre_origin}; physical latency unmeasured",audio.snapshot(),input.counters());
        match audio.last_render_report(){Some(report)=>println!("last successful typed core RenderReport={report:?}; core execution distinct from native delivery/physical sound"),None=>println!("last successful core RenderReport unavailable; no zero observation substituted")}
        if let Err(error) = &stop {
            eprintln!("CoreAudio stop error (existing context-retention policy): {error}");
        }
        if let Err(error) = &close {
            eprintln!("IOHID close error: {error}");
        }
        outcome?;
        stop?;
        close?;
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
            domain: ClockDomainId(2),
            timestamp: Timestamp::from_nanos(n),
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
        assert!(shift_bgm(
            AudioCommand::Play {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::from_nanos(i64::MAX),
                gain: 1.0
            },
            1
        )
        .is_err());
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
        assert!(estimated_origin(
            ClockPair {
                source: point(i64::MAX),
                target: point(i64::MIN)
            },
            point(0)
        )
        .is_err());
    }
    #[test]
    fn input_epoch_future_and_lag_backlog_guards_keep_original_time() {
        assert!(!input_in_epoch(point(9), point(20), point(10)).unwrap());
        assert!(input_in_epoch(point(10), point(20), point(10)).unwrap());
        assert!(input_in_epoch(point(21), point(20), point(10)).is_err());
        assert!(validate_input_chronology(point(100), point(100)).is_ok());
        assert!(validate_input_chronology(point(101), point(100)).is_ok());
        assert!(validate_input_chronology(point(99), point(100)).is_err());
        assert!(validate_input_chronology(
            ClockPoint {
                domain: ClockDomainId(3),
                timestamp: Timestamp::from_nanos(100)
            },
            point(100)
        )
        .is_err());

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
}
