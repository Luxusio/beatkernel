//! Real BMS/WAV assets, physical keyboard acquisition and explicit WASAPI/ASIO output.
#[cfg(any(target_os = "windows", test))]
use beatkernel::audio::AudioCommand;
#[cfg(test)]
use beatkernel::time::Timestamp;
use beatkernel::{audio::AudioLimits, time::Duration};
use beatkernel_platform::audio::{BufferRequest, PeriodRequest, SharedPeriodPolicy};
use std::{
    collections::{BTreeMap, HashSet},
    error::Error,
    path::PathBuf,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Backend {
    Wasapi,
    Asio,
}
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[cfg(test)]
fn park_pause_input(
    events: &mut std::collections::VecDeque<beatkernel::input::PhysicalInputEvent>,
    event: beatkernel::input::PhysicalInputEvent,
) -> Result<()> {
    if events.len() >= 65536 {
        return Err("pause acquisition buffer exhausted; explicit cleanup required".into());
    }
    events.try_reserve(1)?;
    events.push_back(event);
    Ok(())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AsioView {
    Native,
    Bits32,
    Bits64,
}
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
struct Options {
    backend: Backend,
    #[cfg_attr(not(feature = "asio-sdk"), allow(dead_code))]
    asio_view: Option<AsioView>,
    #[cfg_attr(not(feature = "asio-sdk"), allow(dead_code))]
    output_channels: Option<Vec<u32>>,
    #[cfg_attr(not(feature = "asio-sdk"), allow(dead_code))]
    asio_timer_error: u64,
    #[cfg_attr(not(feature = "asio-sdk"), allow(dead_code))]
    asio_drift_error: u64,
    #[cfg_attr(not(feature = "asio-sdk"), allow(dead_code))]
    asio_latency_error: u64,
    #[cfg_attr(not(feature = "asio-sdk"), allow(dead_code))]
    asio_anchor_age: u64,
    chart: PathBuf,
    record_replay: Option<PathBuf>,
    replay_max_records: usize,
    replay_max_bytes: usize,
    device: String,
    keyboard_path: Option<String>,
    local_players: Vec<(beatkernel_bms_runtime::local_players::PlayerId, String)>,
    advance_lag: i64,
    exclusive: bool,
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
    voices: usize,
    mono_stereo: bool,
    buffer: BufferRequest,
    period: PeriodRequest,
    shared: SharedPeriodPolicy,
}
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn selected_keyboard<'a>(
    path: Option<&str>,
    devices: impl IntoIterator<Item = (&'a str, u64, usize)>,
) -> Result<Option<(u64, usize)>> {
    let Some(path) = path else {
        return Ok(None);
    };
    let mut matching = devices
        .into_iter()
        .filter(|(candidate, _, _)| *candidate == path);
    let (_, id, handle) = matching
        .next()
        .ok_or("explicit keyboard path is not attached")?;
    if matching.next().is_some() {
        return Err("explicit keyboard path matches multiple attachments".into());
    }
    Ok(Some((id, handle)))
}
fn size(value: &str) -> Result<Option<(bool, u64)>> {
    if value == "default" {
        return Ok(None);
    }
    let (kind, count) = value
        .split_once(':')
        .ok_or("size must be default, frames:N or ns:N")?;
    let count: u64 = count.parse()?;
    if count == 0 {
        return Err("size must be positive".into());
    }
    match kind {
        "frames" => {
            u32::try_from(count)?;
            Ok(Some((true, count)))
        }
        "ns" => {
            i64::try_from(count)?;
            Ok(Some((false, count)))
        }
        _ => Err("size must be default, frames:N or ns:N".into()),
    }
}
fn local_assignment(
    value: &str,
) -> Result<(beatkernel_bms_runtime::local_players::PlayerId, String)> {
    let (id, path) = value
        .split_once(':')
        .ok_or("local player requires ID:INTERFACE_PATH")?;
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("local player ID requires positive ASCII decimal u32".into());
    }
    let id: u32 = id.parse()?;
    if id == 0
        || path.is_empty()
        || path.len() > 4096
        || path
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
    {
        return Err(
            "local player requires positive u32 ID and bounded nonempty interface path".into(),
        );
    }
    Ok((
        beatkernel_bms_runtime::local_players::PlayerId(id),
        path.to_owned(),
    ))
}
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
impl Options {
    fn playback_end(&self, sample_rate: u32) -> Result<Option<u64>> {
        use beatkernel_bms_runtime::{practice::PracticeStart, practice_loop::PracticeLoop};
        self.end_ns
            .map(|end| {
                let start = PracticeStart::from_nanoseconds(self.start_ns)?;
                Ok(
                    PracticeLoop::new(start, PracticeStart::from_nanoseconds(end)?)?
                        .playback_end_frame(
                            start,
                            Duration::from_nanos(self.preroll),
                            sample_rate,
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
    let mut backend = Backend::Wasapi;
    let mut asio_view = None;
    let mut output_channels = None;
    let mut asio_system_clock = false;
    let (mut asio_timer_error, mut asio_drift_error, mut asio_latency_error) = (None, None, None);
    let mut asio_anchor_age = 1_000_000_000u64;
    let mut chart = None;
    let mut device = None;
    let mut keyboard_path = None;
    let mut local_players = Vec::new();
    let mut advance_lag = 2_000_000i64;
    let mut exclusive = None;
    let mut seconds = None;
    let mut bindings = BTreeMap::new();
    let mut key_usages = HashSet::new();
    let mut early = 150_000_000i64;
    let mut late = 150_000_000i64;
    let mut offset = 0i64;
    let mut preroll = 3_000_000_000i64;
    let mut chart_seed = 0u64;
    let mut start_ns = 0i64;
    let mut end_ns = None;
    let mut bgm_lookahead = 3_000_000_000i64;
    let mut voices = 256usize;
    let mut mono_stereo = false;
    let mut buffer = BufferRequest::DeviceDefault;
    let mut period = PeriodRequest::DeviceDefault;
    let mut shared = SharedPeriodPolicy::EnginePeriod;
    let mut seen = HashSet::new();
    let mut record_replay = None;
    let mut replay_max_records = 1_000_000usize;
    let mut replay_max_bytes = 64 * 1024 * 1024usize;
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        let value = iter.next().ok_or("each option requires a value")?;
        if !matches!(flag.as_str(), "--bind" | "--local-player") && !seen.insert(flag.as_str()) {
            return Err(format!("duplicate option {flag}").into());
        }
        match flag.as_str() {
            "--local-player" => {
                if local_players.len() == beatkernel_bms_runtime::local_players::MAX_LOCAL_PLAYERS {
                    return Err("local player count exceeds 64".into());
                }
                let assignment = local_assignment(value)?;
                if local_players
                    .iter()
                    .any(|(id, path)| *id == assignment.0 || *path == assignment.1)
                {
                    return Err("local player IDs and interface paths must be distinct".into());
                }
                local_players.push(assignment);
            }
            "--advance-lag-ns" => {
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err("advance lag requires unsigned ASCII decimal nanoseconds".into());
                }
                advance_lag = value.parse()?;
                if !(0..=1_000_000_000).contains(&advance_lag) {
                    return Err("advance lag must be 0..1000000000ns".into());
                }
            }
            "--keyboard-path" => {
                if value.is_empty()
                    || value.len() > 4096
                    || value
                        .chars()
                        .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
                {
                    return Err(
                        "keyboard path must be nonempty, bounded and contain no controls".into(),
                    );
                }
                keyboard_path = Some(value.clone());
            }
            "--backend" => {
                backend = match value.as_str() {
                    "wasapi" => Backend::Wasapi,
                    "asio" => Backend::Asio,
                    _ => return Err("backend must be wasapi or asio".into()),
                }
            }
            "--asio-view" => {
                asio_view = Some(match value.as_str() {
                    "native" => AsioView::Native,
                    "32" => AsioView::Bits32,
                    "64" => AsioView::Bits64,
                    _ => return Err("ASIO view must be native, 32 or 64".into()),
                })
            }
            "--output-channels" => {
                let mut selected = Vec::new();
                for token in value.split(',') {
                    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_digit()) {
                        return Err("invalid ASIO channel token".into());
                    }
                    let index: u32 = token.parse()?;
                    if index > i32::MAX as u32 || selected.contains(&index) || selected.len() == 32
                    {
                        return Err("invalid or duplicate ASIO output channel".into());
                    }
                    selected.push(index);
                }
                output_channels = Some(selected);
            }
            "--asio-system-clock" => {
                if value != "multimedia" {
                    return Err("ASIO system clock must be explicitly multimedia".into());
                }
                asio_system_clock = true;
            }
            "--asio-timer-error-ns" | "--asio-drift-error-ns" | "--asio-latency-error-ns" => {
                if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("ASIO error bounds must be nonnegative decimal nanoseconds".into());
                }
                let error: u64 = value.parse()?;
                i64::try_from(error)?;
                match flag.as_str() {
                    "--asio-timer-error-ns" => asio_timer_error = Some(error),
                    "--asio-drift-error-ns" => asio_drift_error = Some(error),
                    _ => asio_latency_error = Some(error),
                }
            }
            "--asio-anchor-age-ns" => {
                asio_anchor_age = value.parse()?;
                if asio_anchor_age == 0 || asio_anchor_age >= (1u64 << 31) * 1_000_000 {
                    return Err("ASIO anchor age must be positive and below half timer wrap".into());
                }
            }

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
            "--device" if !value.is_empty() => device = Some(value.clone()),
            "--mode" => {
                exclusive = Some(match value.as_str() {
                    "shared" => false,
                    "exclusive" => true,
                    _ => return Err("mode must be shared or exclusive".into()),
                })
            }
            "--seconds" => {
                let n = value.parse::<u64>()?;
                if !(1..=3600).contains(&n) {
                    return Err("seconds must be 1..3600".into());
                }
                seconds = Some(n);
            }
            "--bind" => {
                let (channel, usage) = value
                    .split_once(':')
                    .ok_or("binding must be channelHEX:HIDusageHEX")?;
                let channel = u8::from_str_radix(channel, 16)?;
                let usage = u16::from_str_radix(usage, 16)?;
                if !matches!(channel, 0x11..=0x19 | 0x21..=0x29) || usage == 0 {
                    return Err(
                        "binding needs visible BMS channel and nonzero keyboard HID usage".into(),
                    );
                }
                if bindings.insert(channel, usage).is_some() || !key_usages.insert(usage) {
                    return Err("duplicate bound channel or keyboard usage".into());
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
            "--end-ns" => {
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err("end-ns must be unsigned decimal nanoseconds".into());
                }
                end_ns = Some(value.parse::<i64>()?);
            }
            "--preroll-ns" => {
                preroll = value.parse()?;
                if !(0..=10_000_000_000).contains(&preroll) {
                    return Err("preroll must be 0..10000000000 ns".into());
                }
            }
            "--voices" => voices = value.parse()?,
            "--channel-policy" => {
                mono_stereo = match value.as_str() {
                    "exact" => false,
                    "mono-stereo" => true,
                    _ => return Err("channel policy must be exact or mono-stereo".into()),
                }
            }
            "--buffer" => {
                buffer = match size(value)? {
                    None => BufferRequest::DeviceDefault,
                    Some((true, n)) => BufferRequest::Frames(u32::try_from(n)?),
                    Some((false, n)) => {
                        BufferRequest::Duration(Duration::from_nanos(i64::try_from(n)?))
                    }
                }
            }
            "--period" => {
                period = match size(value)? {
                    None => PeriodRequest::DeviceDefault,
                    Some((true, n)) => PeriodRequest::Frames(u32::try_from(n)?),
                    Some((false, n)) => {
                        PeriodRequest::Duration(Duration::from_nanos(i64::try_from(n)?))
                    }
                }
            }
            "--shared-policy" => {
                shared = match value.as_str() {
                    "engine" => SharedPeriodPolicy::EnginePeriod,
                    "legacy" => SharedPeriodPolicy::DeviceDefault,
                    _ => return Err("shared policy must be engine or legacy".into()),
                }
            }
            _ => return Err(format!("unknown or empty option {flag}").into()),
        }
    }
    let mut device = device.ok_or("explicit --device required")?;
    let exclusive = if backend == Backend::Wasapi {
        if seen.iter().any(|flag| flag.starts_with("--asio-")) || output_channels.is_some() {
            return Err("ASIO flags require --backend asio".into());
        }
        exclusive.ok_or("explicit --mode required")?
    } else {
        if exclusive.is_some()
            || seen.contains("--period")
            || seen.contains("--shared-policy")
            || matches!(buffer, BufferRequest::Duration(_))
        {
            return Err("ASIO rejects mode, period, shared policy and nanosecond buffers".into());
        }
        if asio_view.is_none()
            || output_channels.is_none()
            || !asio_system_clock
            || asio_timer_error.is_none()
            || asio_drift_error.is_none()
            || asio_latency_error.is_none()
        {
            return Err("ASIO requires view, output channels, multimedia declaration and all three explicit error bounds".into());
        }
        let b = device.as_bytes();
        if b.len() != 38
            || b[0] != b'{'
            || b[37] != b'}'
            || b[1..37].iter().enumerate().any(|(i, b)| {
                if [8, 13, 18, 23].contains(&i) {
                    *b != b'-'
                } else {
                    !b.is_ascii_hexdigit()
                }
            })
            || !b[1..37].iter().any(|b| b.is_ascii_hexdigit() && *b != b'0')
        {
            return Err("ASIO requires a nonzero braced UUID CLSID".into());
        }
        let zero = beatkernel::time::ClockPoint {
            domain: beatkernel::time::ClockDomainId(1),
            timestamp: beatkernel::time::Timestamp::ZERO,
        };
        beatkernel_platform::audio::asio::MultimediaClockAnchor::new(
            0,
            zero,
            zero,
            asio_anchor_age,
            asio_timer_error.unwrap(),
            asio_drift_error.unwrap(),
        )?;
        device.make_ascii_uppercase();
        if let BufferRequest::Frames(frames) = buffer {
            if frames as usize > AudioLimits::MAX_RENDER_FRAMES {
                return Err("ASIO buffer exceeds core render ceiling".into());
            }
        }
        false
    };
    if exclusive && seen.contains("--shared-policy") {
        return Err("shared-policy is unavailable in exclusive mode".into());
    }
    if !exclusive
        && shared == SharedPeriodPolicy::DeviceDefault
        && period != PeriodRequest::DeviceDefault
    {
        return Err("legacy shared initialization requires --period default".into());
    }
    if end_ns.is_some_and(|end| end <= start_ns) {
        return Err("end-ns must be strictly after start-ns".into());
    }
    if early < 0 || late < 0 || !(1..=AudioLimits::MAX_VOICES).contains(&voices) {
        return Err("windows must be nonnegative; voices must be 1..4096".into());
    }
    if !local_players.is_empty() && (local_players.len() < 2 || keyboard_path.is_some()) {
        return Err(
            "local play requires 2..64 assignments and no solo keyboard-path override".into(),
        );
    }
    Ok(Options {
        keyboard_path,
        local_players,
        advance_lag,
        backend,
        asio_view,
        output_channels,
        asio_timer_error: asio_timer_error.unwrap_or(0),
        asio_drift_error: asio_drift_error.unwrap_or(0),
        asio_latency_error: asio_latency_error.unwrap_or(0),
        asio_anchor_age,
        record_replay,
        replay_max_records,
        replay_max_bytes,
        chart: chart.ok_or("explicit --chart required")?,
        device,
        exclusive,
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
        voices,
        mono_stereo,
        buffer,
        period,
        shared,
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

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn calibration_extent(seconds: u64, preroll: i64) -> Result<i64> {
    let nanos = i64::try_from(seconds)?
        .checked_mul(1_000_000_000)
        .and_then(|n| n.checked_add(preroll))
        .and_then(|n| n.checked_add(3_000_000_000))
        .ok_or("calibration duration/preroll overflow")?;
    Ok(nanos)
}

#[cfg(target_os = "windows")]
struct BgmSession(beatkernel_bms_runtime::bgm::BgmFeeder);
#[cfg(target_os = "windows")]
impl std::ops::Deref for BgmSession {
    type Target = beatkernel_bms_runtime::bgm::BgmFeeder;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
#[cfg(target_os = "windows")]
impl std::ops::DerefMut for BgmSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
#[cfg(target_os = "windows")]
impl Drop for BgmSession {
    fn drop(&mut self) {
        println!(
            "BGM feeder config={:?}; final admission summary={:?}; admission does not prove execution/native delivery/acoustic output",
            self.config(),
            self.report()
        );
    }
}
#[cfg(target_os = "windows")]
fn feed_rendered(
    bgm: &mut BgmSession,
    report: Option<beatkernel::audio::RenderReport>,
    admit: impl FnMut(AudioCommand) -> std::result::Result<(), beatkernel::audio::CommandPushError>,
) -> Result<()> {
    if let Some(report) = report.filter(|report| !report.paused) {
        let end = report
            .playback_start_frame
            .checked_add(u64::try_from(report.playback_frames)?)
            .ok_or("BGM render cursor overflow")?;
        bgm.feed(end, 256, admit)?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
struct DeliverySession(beatkernel::telemetry::InputDeliveryTelemetry);
#[cfg(target_os = "windows")]
impl std::ops::Deref for DeliverySession {
    type Target = beatkernel::telemetry::InputDeliveryTelemetry;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
#[cfg(target_os = "windows")]
impl std::ops::DerefMut for DeliverySession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
#[cfg(target_os = "windows")]
impl Drop for DeliverySession {
    fn drop(&mut self) {
        let observed = self.observed_events();
        match self.summary() {
            Some(summary) => println!(
                "QPC RECEIPT-to-runtime software delivery age: observed_events={observed}; retained samples={} p50={}ns p95={}ns p99={}ns max={}ns; HOST={:?}, capacity={}; separate from CPU processing; physical input-to-sound unknown",
                summary.samples,
                summary.p50_ns,
                summary.p95_ns,
                summary.p99_ns,
                summary.max_ns,
                self.domain(),
                self.capacity()
            ),
            None => println!(
                "QPC RECEIPT-to-runtime software delivery age: observed_events={observed}; retained summary unavailable; no zero observation substituted; separate from CPU processing; physical input-to-sound unknown"
            ),
        }
    }
}

#[cfg(target_os = "windows")]
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
    let (competition, native) =
        beatkernel_bms_runtime::competition_live::CompetitionOptions::extract(args)?;
    let options = parse(&native)?;
    validate_finite_modes(&options, &competition)
}

fn validate_finite_modes(
    options: &Options,
    competition: &beatkernel_bms_runtime::competition_live::CompetitionOptions,
) -> Result<()> {
    if !options.local_players.is_empty() && competition.network.is_some() {
        return Err("network competition currently supports one local participant only".into());
    }
    Ok(())
}

#[cfg(any(target_os = "windows", test))]
fn finite_session_done(
    end_ns: Option<i64>,
    presented: Option<beatkernel::time::ClockPoint>,
    watermark: beatkernel::time::ClockPoint,
    song: beatkernel::time::Timestamp,
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
            "Local play: repeat --local-player ID:EXACT_INTERFACE_PATH for 2..64 distinct keyboards, without --keyboard-path. Stable positive u32 IDs are preserved in GUI scores and .p<ID>.bkr replay files. --advance-lag-ns 0..1000000000 (default 2000000) controls the common input frontier. Network + local groups is unsupported; saved ghosts are per-player. Native commands compose the graphical player's actual runtime."
        );
        println!(
            "windows_bms --chart PATH --device EXACT_ID [--backend wasapi|asio] --mode shared|exclusive [--seconds N] --bind channelHEX:HIDusageHEX [--bind ...]\nASIO instead requires --asio-view native|32|64 --output-channels 0,1 --asio-system-clock multimedia --asio-timer-error-ns N --asio-drift-error-ns N --asio-latency-error-ns N; optional --asio-anchor-age-ns N (default1000000000), exact --buffer frames:N or preferred default. ASIO rejects mode/period/shared-policy and ns buffers; WASAPI rejects ASIO flags. ASIO requires sample asio-sdk, SDK/MSVC toolchain and explicitly selected trusted installed driver. Error bounds are caller estimates, not physical guarantees.\nOptions: --record-replay PATH --replay-max-records N --replay-max-bytes N --bgm-lookahead-ns N --buffer default|frames:N|ns:N --period default|frames:N|ns:N --shared-policy engine|legacy --channel-policy exact|mono-stereo --voices N --early-ns N --late-ns N --input-offset-ns N --chart-seed DECIMAL_U64 --start-ns N --end-ns N --preroll-ns N\nBounds: seconds 1..3600, voices 1..4096, preroll 0..10000000000 ns, BGM lookahead positive i64 ns. Defaults: chart seed0, replay disabled, max records 1000000, max bytes 67108864, BGM lookahead3000000000ns, buffer/period default, shared engine, exact channels, voices256, early/late150000000ns, offset0, preroll3000000000ns. Optional --end-ns unsigned strictly after start completes a native-presented, input-drained finite prefix for solo or local WASAPI/SDK-enabled ASIO; ASIO waits for the actual crossing block upper presentation interval. Solo network peers must agree on the same finite section endpoint; local groups remain offline. Missing --seconds plays the full song through terminal judging and reported native audio presentation; --seconds is a diagnostic loop cutoff after calibration, including remaining preroll. Bind every used BMS lane explicitly; Optional --keyboard-path EXACT_INTERFACE_PATH selects one physical keyboard; omitted accepts any physical keyboard. Explicit device removal fails the session. Focused native window. Actual supported BMS and WAV assets; no synthetic input. Physical latency unmeasured."
        );
        return Ok(());
    }
    let options = parse(&args)?;
    validate_finite_modes(&options, &competition_options)?;
    #[cfg(target_os = "windows")]
    {
        native::run(options, competition_options)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (options, competition_options);
        Err("windows_bms native playback requires Windows".into())
    }
}

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use beatkernel::{
        audio::{Mixer, MixerConfig, PcmLimits, command_queue, command_queue_with_start_gate},
        input::{Binding, BindingMap, DeviceSelector, GameControlId, PhysicalControlId},
        judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
        time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp},
        transport::Rate,
    };
    use beatkernel_bms_runtime::local_runtime::SoloRuntime as Runtime;
    use beatkernel_bms_runtime::{ChannelPolicy, load_prepared_with_seed};
    use beatkernel_bms_runtime::{
        playback_pause::NativePause,
        player::{self},
    };
    use beatkernel_platform::{
        audio::{
            AudioOutputStream, AudioStreamStatus,
            presentation::{
                PresentationError, WasapiPresentationClock,
                discipline::{
                    DisciplineConfig, DisciplineError, ObservationAdmission, PresentationDiscipline,
                },
            },
        },
        windows::{
            audio::WasapiStream,
            clock::QpcClock,
            input::{RawInputRegistration, RawInputUsage, WindowsInput},
        },
    };
    use std::{
        io, ptr,
        time::{Duration as WallDuration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::{HINSTANCE, HWND},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GIDC_ARRIVAL,
            GIDC_REMOVAL, MSG, PM_REMOVE, PeekMessageW, RegisterClassW, TranslateMessage,
            UnregisterClassW, WM_CLOSE, WM_INPUT, WM_INPUT_DEVICE_CHANGE, WM_QUIT, WNDCLASSW,
            WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        },
    };
    pub(super) const HOST: ClockDomainId = ClockDomainId(1);
    pub(super) const OUTPUT: ClockDomainId = ClockDomainId(2);
    pub(super) struct ExplicitDomains;
    impl ClockMapper for ExplicitDomains {
        fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
            None
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Unknown
        }
    }
    pub(super) struct Window {
        pub(super) hwnd: HWND,
        instance: HINSTANCE,
        class: Vec<u16>,
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        if message == WM_CLOSE {
            // SAFETY: posts only to this thread. Preserve the live window until
            // the pump closes registration, including synchronously sent close.
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            }
            return 0;
        }
        // SAFETY: the native caller supplies window/message values. This
        // callback owns no Rust state and cannot unwind across the ABI.
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    impl Window {
        fn new() -> Result<Self> {
            Self::with_visibility(!beatkernel_bms_runtime::player::attached(), "Input")
        }
        #[cfg(feature = "asio-sdk")]
        pub(super) fn hidden() -> Result<Self> {
            Self::with_visibility(false, "AsioSysref")
        }
        fn with_visibility(visible: bool, role: &str) -> Result<Self> {
            let class: Vec<u16> = format!("BeatKernelBms{}{}", std::process::id(), role)
                .encode_utf16()
                .chain(Some(0))
                .collect();
            // SAFETY: documented null query gets this executable's module.
            let instance = unsafe { GetModuleHandleW(ptr::null()) };
            if instance.is_null() {
                return Err(io::Error::last_os_error().into());
            }
            let descriptor = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(window_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: ptr::null_mut(),
                hCursor: ptr::null_mut(),
                hbrBackground: ptr::null_mut(),
                lpszMenuName: ptr::null(),
                lpszClassName: class.as_ptr(),
            };
            // SAFETY: initialized class, terminated live name, native callback
            // without any Rust callback state or unwind across the ABI.
            if unsafe { RegisterClassW(&descriptor) } == 0 {
                return Err(io::Error::last_os_error().into());
            }
            let mut window = Self {
                hwnd: ptr::null_mut(),
                instance,
                class,
            };
            let title: Vec<u16> = "BeatKernel BMS: focus this window; see console bindings/results"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            // SAFETY: live registered class/module and UTF-16 strings, no menu,
            // parent or user-data pointers. Drop owns partial failure cleanup.
            window.hwnd = unsafe {
                CreateWindowExW(
                    0,
                    window.class.as_ptr(),
                    title.as_ptr(),
                    if visible {
                        WS_OVERLAPPEDWINDOW | WS_VISIBLE
                    } else {
                        0
                    },
                    100,
                    100,
                    640,
                    240,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    instance,
                    ptr::null(),
                )
            };
            if window.hwnd.is_null() {
                return Err(io::Error::last_os_error().into());
            }
            Ok(window)
        }
    }

    impl Drop for Window {
        fn drop(&mut self) {
            // SAFETY: current thread owns this window/class; registration guard
            // is constructed after Window and cleaned before destruction.
            unsafe {
                if !self.hwnd.is_null() {
                    DestroyWindow(self.hwnd);
                }
                UnregisterClassW(self.class.as_ptr(), self.instance);
            }
        }
    }

    pub(super) fn schedule_wasapi(stream: &WasapiStream, rate: u32) -> Result<ClockPoint> {
        for _ in 0..64 {
            let snapshot = stream.snapshot();
            if snapshot.status != AudioStreamStatus::Running {
                return Err(format!("audio terminated: {:?}", snapshot.status).into());
            }
            if snapshot.telemetry_available {
                let nanos = (u128::from(snapshot.counters.submitted_frames) * 1_000_000_000)
                    .div_ceil(u128::from(rate));
                return Ok(ClockPoint {
                    domain: OUTPUT,
                    timestamp: Timestamp::from_nanos(i64::try_from(nanos)?),
                });
            }
            std::thread::yield_now();
        }
        Err("coherent audio telemetry unavailable".into())
    }

    pub(super) fn presentation_wasapi(
        stream: &WasapiStream,
        extent: i64,
        bgm: &mut BgmSession,
        producer: &mut beatkernel::audio::CommandProducer,
    ) -> Result<WasapiPresentationClock> {
        use beatkernel::time::{CalibrationUncertainty, ClockInterval, ExtrapolationPolicy};
        use beatkernel_platform::audio::AudioClockReadingQuality;
        let deadline = Instant::now() + WallDuration::from_secs(2);
        let mut first = None;
        while Instant::now() < deadline {
            let snapshot = stream.snapshot();
            if snapshot.status != AudioStreamStatus::Running {
                return Err("audio terminated before presentation calibration".into());
            }
            feed_rendered(bgm, snapshot.render, |command| producer.try_push(command))?;
            let usable = snapshot.telemetry_available
                && snapshot.clock.is_some_and(|clock| {
                    clock.position != 0
                        && clock.frequency != 0
                        && clock.host_point.is_some()
                        && clock.reading_quality == AudioClockReadingQuality::Accurate
                });
            if usable {
                if let Some(previous) = first {
                    match WasapiPresentationClock::from_snapshots(
                        previous,
                        snapshot,
                        ClockPoint {
                            domain: OUTPUT,
                            timestamp: Timestamp::ZERO,
                        },
                        ClockInterval {
                            start: Timestamp::ZERO,
                            end: Timestamp::from_nanos(extent),
                        },
                        ExtrapolationPolicy::Bounded {
                            before: Duration::from_nanos(3_000_000_000),
                            after: Duration::from_nanos(extent),
                        },
                        CalibrationUncertainty {
                            observation_error: Duration::from_nanos(100),
                            residual_drift_error: None,
                        },
                    ) {
                        Ok(relation) => return Ok(relation),
                        Err(PresentationError::NonIncreasing) => {}
                        Err(error) => return Err(error.into()),
                    }
                } else {
                    first = Some(snapshot);
                }
            }
            std::thread::sleep(WallDuration::from_millis(1));
        }
        Err("no usable increasing presentation observations within 2 seconds".into())
    }

    pub(super) fn skippable_observation(error: &DisciplineError) -> bool {
        matches!(
            error,
            DisciplineError::Presentation(
                PresentationError::Unavailable
                    | PresentationError::Inaccurate
                    | PresentationError::BeforePresentation
            )
        )
    }
    pub(super) fn observe_wasapi(
        stream: &WasapiStream,
        discipline: &mut PresentationDiscipline,
    ) -> Result<Option<ObservationAdmission>> {
        let snapshot = stream.snapshot();
        if snapshot.status != AudioStreamStatus::Running {
            return Err(format!(
                "native presentation observation terminated: {:?}",
                snapshot.status
            )
            .into());
        }
        match discipline.observe(snapshot) {
            Ok(admission) => Ok(Some(admission)),
            Err(error) if skippable_observation(&error) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    pub(super) fn seed_wasapi(
        stream: &WasapiStream,
        discipline: &mut PresentationDiscipline,
        bgm: &mut BgmSession,
        producer: &mut beatkernel::audio::CommandProducer,
    ) -> Result<()> {
        let deadline = Instant::now() + WallDuration::from_secs(2);
        while Instant::now() < deadline {
            let admission = observe_wasapi(stream, discipline)?;
            feed_rendered(bgm, stream.snapshot().render, |command| {
                producer.try_push(command)
            })?;
            if matches!(
                admission,
                Some(ObservationAdmission::Retained | ObservationAdmission::Progress)
            ) {
                return Ok(());
            }
            std::thread::sleep(WallDuration::from_millis(1));
        }
        Err("no accurate progressing presentation seed within two seconds".into())
    }
    // Own registration and its target together. Created before stream, so early
    // returns drop/join audio before this guard unregisters and destroys Window.
    pub(super) struct AcquisitionWindow {
        pub(super) registration: RawInputRegistration,
        window: Option<Window>,
    }
    impl AcquisitionWindow {
        pub(super) fn new() -> Result<Self> {
            let window = Window::new()?;
            let registration =
                RawInputRegistration::register(window.hwnd as usize, &[RawInputUsage::KEYBOARD])?;
            Ok(Self {
                registration,
                window: Some(window),
            })
        }
        pub(super) fn hwnd(&self) -> HWND {
            self.window.as_ref().expect("live window owner").hwnd
        }
    }
    impl Drop for AcquisitionWindow {
        fn drop(&mut self) {
            if let Err(error) = self.registration.close() {
                eprintln!("Raw Input cleanup failed; retaining native window/class: {error}");
                if let Some(window) = self.window.take() {
                    std::mem::forget(window);
                }
            }
        }
    }
    use beatkernel_bms_runtime::native_gameplay::{
        InputBatch, NativeGameplayConfig, NativeGameplayDevice, NativeGameplayResult,
        NativeGameplaySession, retain_input, run_gameplay,
    };
    use beatkernel_bms_runtime::native_start::{
        MAX_START_INPUT_EVENTS, NativeStartConfig, NativeStartDevice, NativeStartObservation,
        NativeStartResult, start_committed,
    };
    struct GameplayDevice<'a> {
        stream: &'a mut super::live_output::Output,
        input: &'a mut WindowsInput,
        acquisition: &'a AcquisitionWindow,
        clock: &'a QpcClock,
        selected: Option<(u64, usize)>,
        retained: &'a mut std::collections::VecDeque<beatkernel::input::PhysicalInputEvent>,
        last_snapshot: Option<beatkernel_platform::audio::AudioStreamSnapshot>,
    }
    impl NativeGameplayDevice for GameplayDevice<'_> {
        fn observe(&mut self, discipline: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
            match self.stream {
                super::live_output::Output::Wasapi(_) => {
                    if let Some((_, snapshot)) = self.stream.startup_observation(discipline)? {
                        self.last_snapshot = Some(snapshot);
                    }
                }
                #[cfg(feature = "asio-sdk")]
                super::live_output::Output::Asio(_) => {
                    self.stream.observe(discipline)?;
                }
            }
            Ok(())
        }
        fn render_report(
            &mut self,
        ) -> NativeGameplayResult<Option<beatkernel::audio::RenderReport>> {
            self.stream.render_report()
        }
        fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
            Ok(self.clock.sample()?.normalized)
        }
        fn acquire(
            &mut self,
            events: &mut std::collections::VecDeque<beatkernel::input::PhysicalInputEvent>,
        ) -> NativeGameplayResult<InputBatch> {
            let mut count = 0;
            while count < 256 {
                let Some(event) = self.retained.pop_front() else {
                    break;
                };
                retain_input(events, event)?;
                count += 1;
            }
            if count == 256 {
                return Ok(InputBatch {
                    backlog: true,
                    closed: false,
                });
            }
            read_messages(
                self.input,
                self.acquisition,
                self.selected,
                256 - count,
                |event| retain_input(events, event),
            )
        }
        fn observe_end(
            &mut self,
            end: &mut beatkernel_bms_runtime::native_end::NativeEnd,
            discipline: &PresentationDiscipline,
            report: Option<beatkernel::audio::RenderReport>,
        ) -> NativeGameplayResult<Option<beatkernel_bms_runtime::native_end::EndBoundary>> {
            self.stream.observe_end(end, discipline, report)
        }
        fn seed_resume(
            &mut self,
            discipline: &mut PresentationDiscipline,
            reference: beatkernel::time::ClockPair,
        ) -> NativeGameplayResult<()> {
            let snapshot = self
                .last_snapshot
                .ok_or("original WASAPI resume snapshot unavailable")?;
            discipline.observe(snapshot)?;
            if discipline.latest_pair() != Some(reference) {
                return Err("WASAPI resume snapshot differs from accepted reference".into());
            }
            Ok(())
        }
        fn fallback_schedule(&mut self, rate: u32) -> NativeGameplayResult<ClockPoint> {
            self.stream.schedule(rate)
        }
    }
    struct StartupDevice<'a> {
        stream: &'a mut super::live_output::Output,
        input: &'a mut WindowsInput,
        acquisition: &'a AcquisitionWindow,
        clock: &'a QpcClock,
        selected: Option<(u64, usize)>,
        pre_origin: &'a mut u64,
        retained: &'a mut std::collections::VecDeque<beatkernel::input::PhysicalInputEvent>,
        physical: PresentationDiscipline,
    }
    impl NativeStartDevice for StartupDevice<'_> {
        type Evidence = beatkernel_platform::audio::AudioStreamSnapshot;
        fn start(&mut self) -> NativeStartResult<()> {
            self.stream.start()
        }
        fn service_input(&mut self, retain: bool) -> NativeStartResult<bool> {
            startup_messages(
                self.input,
                self.acquisition,
                self.selected,
                self.pre_origin,
                if retain {
                    Some(&mut *self.retained)
                } else {
                    None
                },
            )
        }
        fn observe(&mut self) -> NativeStartResult<Option<NativeStartObservation<Self::Evidence>>> {
            Ok(self
                .stream
                .startup_observation(&mut self.physical)?
                .map(|(pair, evidence)| NativeStartObservation { pair, evidence }))
        }
        fn render_report(&mut self) -> NativeStartResult<Option<beatkernel::audio::RenderReport>> {
            self.stream.render_report()
        }
        fn buffer_frames(&self) -> NativeStartResult<u32> {
            self.stream.startup_buffer_frames()
        }
        fn host_now(&self) -> NativeStartResult<ClockPoint> {
            Ok(self.clock.sample()?.normalized)
        }
    }
    fn startup_messages(
        input: &mut WindowsInput,
        acquisition: &AcquisitionWindow,
        selected: Option<(u64, usize)>,
        pre_origin: &mut u64,
        mut retained: Option<
            &mut std::collections::VecDeque<beatkernel::input::PhysicalInputEvent>,
        >,
    ) -> Result<bool> {
        let batch = read_messages(input, acquisition, selected, 256, |event| {
            if let Some(events) = retained.as_deref_mut() {
                if events.len() >= MAX_START_INPUT_EVENTS {
                    return Err("startup Raw Input buffer exhausted; restart required".into());
                }
                events.push_back(event);
            } else {
                *pre_origin = pre_origin.saturating_add(1);
            }
            Ok(())
        })?;
        Ok(!batch.closed)
    }
    fn read_messages(
        input: &mut WindowsInput,
        acquisition: &AcquisitionWindow,
        selected: Option<(u64, usize)>,
        limit: usize,
        mut admit: impl FnMut(beatkernel::input::PhysicalInputEvent) -> Result<()>,
    ) -> Result<InputBatch> {
        if player::cancelled() {
            return Ok(InputBatch {
                backlog: false,
                closed: true,
            });
        }
        // SAFETY: initialized native message storage, owned by this game thread.
        let mut message: MSG = unsafe { std::mem::zeroed() };
        for _ in 0..limit {
            // SAFETY: live writable output on the message owner.
            if unsafe { PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) } == 0 {
                return Ok(InputBatch {
                    backlog: false,
                    closed: false,
                });
            }
            if message.message == WM_QUIT || message.message == WM_CLOSE {
                return Ok(InputBatch {
                    backlog: false,
                    closed: true,
                });
            }
            if message.hwnd == acquisition.hwnd() && message.message == WM_INPUT {
                let acquired = input.read_raw_input(message.lParam as usize, Some(message.time));
                if message.wParam & 0xff == 0 {
                    // SAFETY: foreground Raw Input cleanup occurs once, even on decode failure.
                    unsafe {
                        DefWindowProcW(
                            message.hwnd,
                            message.message,
                            message.wParam,
                            message.lParam,
                        );
                    }
                }
                for event in acquired?.input.events {
                    if selected.is_some_and(|(id, _)| event.meta().source.0 != id) {
                        continue;
                    }
                    admit(event)?;
                }
                continue;
            }
            if message.hwnd == acquisition.hwnd() && message.message == WM_INPUT_DEVICE_CHANGE {
                match message.wParam as u32 {
                    GIDC_ARRIVAL => {
                        input.attach_device(message.lParam as usize)?;
                    }
                    GIDC_REMOVAL => {
                        if selected.is_some_and(|(_, handle)| handle == message.lParam as usize) {
                            return Err("selected keyboard detached during acquisition".into());
                        }
                        input.remove_device(message.lParam as usize);
                    }
                    _ => {}
                }
            }
            // SAFETY: real message and stateless owning-window procedure.
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        Ok(InputBatch {
            backlog: true,
            closed: false,
        })
    }
    pub(super) fn run(
        options: Options,
        competition_options: beatkernel_bms_runtime::competition_live::CompetitionOptions,
    ) -> Result<()> {
        if !options.local_players.is_empty() {
            return super::local_native::run(options, competition_options);
        }
        let pause_supported =
            options.backend == Backend::Wasapi && competition_options.network.is_none();
        let clock = QpcClock::new(HOST)?;
        // Declared before device owners so every exit reports after their cleanup.
        let mut delivery = DeliverySession(beatkernel::telemetry::InputDeliveryTelemetry::new(
            4096, HOST,
        )?);
        if options.backend == Backend::Asio && !cfg!(feature = "asio-sdk") {
            return Err("ASIO requires sample feature asio-sdk, caller SDK and MSVC compiler; no files loaded".into());
        }
        let setup = super::live_output::Setup::new(&options, clock)?;
        let pcm = setup.format();
        let output_origin = ClockPoint {
            domain: OUTPUT,
            timestamp: Timestamp::ZERO,
        };
        let playback_end = options.playback_end(pcm.sample_rate())?;
        let mut pause = NativePause::new(output_origin, HOST, pcm.sample_rate())?;
        if let Some(end) = playback_end {
            pause = pause.with_playback_end_frame(end)?;
        }
        let mut native_end = playback_end
            .map(|end| {
                beatkernel_bms_runtime::native_end::NativeEnd::new(
                    output_origin,
                    HOST,
                    pcm.sample_rate(),
                    end,
                )
            })
            .transpose()?;
        let prepared = load_prepared_with_seed(
            &options.chart,
            pcm,
            PcmLimits::new(64 * 1024 * 1024, 256 * 1024 * 1024, 1295)?,
            if options.mono_stereo {
                ChannelPolicy::MonoToStereo
            } else {
                ChannelPolicy::Exact
            },
            options.chart_seed,
        )?;
        let (prepared, section) = beatkernel_bms_runtime::section_start::prepare_at(
            prepared,
            Timestamp::from_nanos(options.start_ns),
            PcmLimits::new(64 * 1024 * 1024, 256 * 1024 * 1024, 1295)?,
        )?;
        println!("prepared practice section={section:?}");
        let mut completion = if options.end_ns.is_none() {
            Some(beatkernel_bms_runtime::completion::SongCompletion::prepare(
                &prepared,
                options.late,
                options.offset,
                options.preroll,
                OUTPUT,
            )?)
        } else {
            None
        };
        for warning in &prepared.source.warnings {
            eprintln!(
                "BMS parser warning line {}: {}",
                warning.line, warning.message
            );
        }
        for note in &prepared.source.notes {
            if !options.bindings.contains_key(&note.lane.channel()) {
                return Err(format!(
                    "missing --bind for used BMS channel {:02X}",
                    note.lane.channel()
                )
                .into());
            }
        }
        let mut acquisition = AcquisitionWindow::new()?;
        let mut input = WindowsInput::new(clock);
        let devices = input.enumerate_devices()?;
        let selected = selected_keyboard(
            options.keyboard_path.as_deref(),
            devices
                .iter()
                .filter(|d| d.kind == beatkernel_platform::raw_input::RawDeviceKind::Keyboard)
                .map(|d| {
                    (
                        d.interface_path.as_str(),
                        d.descriptor.runtime_id.0,
                        d.handle,
                    )
                }),
        )?;
        let keyboard_selector = selected.map_or(DeviceSelector::Any, |(id, _)| {
            DeviceSelector::Exact(beatkernel::input::DeviceId(id))
        });
        let bindings =
            BindingMap::from_bindings(options.bindings.iter().map(|(&channel, &usage)| Binding {
                device: keyboard_selector,
                physical: PhysicalControlId::keyboard(usage),
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
        const LIVE_SLACK: usize = 1024;
        let capacity = AudioLimits::MAX_COMMANDS;
        let limits = AudioLimits::new(
            capacity,
            options.voices,
            capacity,
            AudioLimits::MAX_RENDER_FRAMES,
            capacity,
        )?;
        let network_start =
            options.backend == Backend::Wasapi && competition_options.network.is_some();
        let (mut producer, consumer) = if network_start {
            command_queue_with_start_gate(capacity)?
        } else {
            command_queue(capacity)?
        };
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
                sample_rate: pcm.sample_rate(),
                preroll: Duration::from_nanos(options.preroll),
                lookahead: Duration::from_nanos(options.bgm_lookahead),
                max_pending: capacity - LIVE_SLACK,
            },
        )?);
        bgm.feed(0, capacity - LIVE_SLACK, |command| {
            producer.try_push(command)
        })?;
        let mixer = Mixer::new(
            {
                let config = MixerConfig::new(pcm, OUTPUT, Timestamp::ZERO, limits);
                playback_end.map_or(config, |end| config.with_playback_end_frame(end))
            },
            prepared.bank,
            consumer,
        )?;
        println!(
            "explicit Any-keyboard bindings={:?}; windows early={}ns late={}ns offset={}ns; channel_policy={} active_voices={} queue/pending={} reserved_live={LIVE_SLACK}",
            options.bindings,
            options.early,
            options.late,
            options.offset,
            if options.mono_stereo {
                "mono-stereo"
            } else {
                "exact"
            },
            options.voices,
            capacity
        );
        let mut stream = setup.open(mixer, &options, clock)?;
        println!("requested/applied native output={:?}", stream.description());
        println!(
            "Focus the BeatKernel BMS native window and play the explicitly bound physical keys. Console prints actual grades and misses."
        );
        println!(
            "section start={}ns preroll={}ns; output zero maps to song={:?}; short startup pairs do not establish long-run clock stability",
            options.start_ns,
            options.preroll,
            options.song_origin()?
        );
        if options.preroll == 0 {
            println!(
                "zero preroll: calibration can consume initial BGM/notes before the gameplay pump"
            );
        }
        let mut capture = None;
        let mut pre_origin_inputs = 0u64;
        let mut startup_inputs = std::collections::VecDeque::with_capacity(MAX_START_INPUT_EVENTS);
        let outcome = (|| -> Result<()> {
            if options.record_replay.is_some() {
                let limits = beatkernel::replay::codec::ReplayCodecLimits::new(
                    options.replay_max_bytes,
                    options.replay_max_records,
                    4096,
                    beatkernel::input::CodecLimits::new(65536, 32768)?,
                )?;
                capture = Some(
                    beatkernel_bms_runtime::replay_capture::LiveReplayCapture::new_at_with_chart_seed(
                        &judge,
                        HOST,
                        limits,
                        Timestamp::from_nanos(options.start_ns),
                        options.chart_seed,
                    )?,
                );
            }
            let (transport, quality, mut discipline, playback_origin) = if network_start {
                let competition = competition
                    .as_mut()
                    .ok_or("network startup owner missing")?;
                let started = {
                    let mut device = StartupDevice {
                        stream: &mut stream,
                        input: &mut input,
                        acquisition: &acquisition,
                        clock: &clock,
                        selected,
                        pre_origin: &mut pre_origin_inputs,
                        retained: &mut startup_inputs,
                        physical: PresentationDiscipline::new(
                            DisciplineConfig::default(),
                            output_origin,
                            HOST,
                            options.song_origin()?,
                        )?,
                    };
                    start_committed(
                        &mut device,
                        competition,
                        &mut producer,
                        &mut pause,
                        &mut native_end,
                        NativeStartConfig {
                            output_origin,
                            sample_rate: pcm.sample_rate(),
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
                let observation = started.observation;
                let origin = started.host_origin;
                let mut discipline = PresentationDiscipline::new_with_playback_origin(
                    DisciplineConfig::default(),
                    output_origin,
                    plan.selected_output(),
                    HOST,
                    options.song_origin()?,
                )?;
                // Retain the exact native snapshot whose accepted pair bracketed presentation.
                discipline.observe(observation.evidence)?;
                let transport = beatkernel::transport::Transport::new(
                    origin.timestamp,
                    options.song_origin()?,
                    Rate::NORMAL,
                );
                println!(
                    "WASAPI applied start={plan:?}; host={origin:?}; physical accuracy unmeasured"
                );
                (
                    transport,
                    ClockMappingQuality::Unknown,
                    discipline,
                    plan.selected_output(),
                )
            } else {
                if let Some(competition) = competition.as_mut() {
                    if !competition.await_network_ready(|| {
                        startup_messages(
                            &mut input,
                            &acquisition,
                            selected,
                            &mut pre_origin_inputs,
                            None,
                        )
                    })? {
                        return Ok(());
                    }
                }
                stream.start()?;
                let (mut transport, quality) = stream.calibrate(
                    &options,
                    calibration_extent(
                        options.seconds.unwrap_or_else(|| {
                            completion.as_ref().map_or(2, |c| c.calibration_seconds())
                        }),
                        options.preroll,
                    )?,
                    &mut bgm,
                    &mut producer,
                )?;
                transport.set_rate(transport.anchor().host_time, Rate::NORMAL)?;
                let mut discipline = PresentationDiscipline::new(
                    DisciplineConfig::default(),
                    output_origin,
                    HOST,
                    options.song_origin()?,
                )?;
                stream.seed(&mut discipline, &mut bgm, &mut producer)?;
                (transport, quality, discipline, output_origin)
            };
            discipline.validate_host(clock.sample()?.normalized)?;
            println!(
                "presentation discipline seed={:?} config={:?} quality={:?}; ongoing continuous transport correction, PCM/BGM rate unchanged",
                discipline.latest_pair(),
                discipline.config(),
                discipline.quality()
            );
            println!(
                "observed output-zero/practice-song anchor={:?}; mapping quality={:?}; keysound scheduling=backend software output frontier/Unknown; physical latency=unmeasured",
                transport.anchor(),
                quality
            );
            let initial_host = transport.anchor().host_time;
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
            if let Some(end) = options.end_ns {
                runtime.set_song_end(Timestamp::from_nanos(end))?;
            }
            let pump = {
                let mut device = GameplayDevice {
                    stream: &mut stream,
                    input: &mut input,
                    acquisition: &acquisition,
                    clock: &clock,
                    selected,
                    retained: &mut startup_inputs,
                    last_snapshot: None,
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
                        pre_origin_inputs: &mut pre_origin_inputs,
                    },
                    NativeGameplayConfig {
                        origin: ClockPoint {
                            domain: HOST,
                            timestamp: initial_host,
                        },
                        stream_origin: output_origin,
                        playback_origin,
                        song_origin: options.song_origin()?,
                        sample_rate: pcm.sample_rate(),
                        end_song: options.end_ns.map(Timestamp::from_nanos),
                        advance_lag: beatkernel::time::Duration::from_nanos(options.advance_lag),
                        seconds: options.seconds,
                        pause_supported,
                        logical_schedule: options.backend == Backend::Wasapi,
                    },
                )
            };
            println!(
                "runtime counters={:?} software processing={:?}",
                runtime.telemetry().counters(),
                runtime.telemetry().processing()
            );
            pump
        })();
        // Both cleanups run before propagating any start/calibration/pump error.
        let stop = stream.stop(); // closes/drains the selected native backend
        let close = acquisition.registration.close();
        println!(
            "pre-output-origin physical inputs ignored without retimestamping={pre_origin_inputs}; physical latency remains unmeasured"
        );
        println!(
            "final audio snapshot={:?}; physical latency=unmeasured",
            stream.description()
        );
        if let Err(error) = &stop {
            eprintln!("native output stop/join error: {error}");
        }
        if let Err(error) = &close {
            eprintln!("Raw Input unregister error: {error}");
        }
        if let Some(competition) = competition.as_mut() {
            competition.finish();
        }
        let save = save_capture(
            capture,
            options.record_replay.as_deref(),
            outcome.is_err() || stop.is_err() || close.is_err(),
        );
        if let Err(error) = &save {
            eprintln!(
                "replay save error after cleanup (valid captured prefix retained until save): {error}"
            );
        }
        outcome?;
        stop?;
        close?;
        save?;
        Ok(())
    }
}

#[cfg(test)]
mod preroll_fixtures {
    use super::*;
    #[test]
    fn finite_options_use_applied_rate_and_reject_unintegrated_owners() {
        let base = arguments(None);
        assert_eq!(parse(&base).unwrap().playback_end(48000).unwrap(), None);
        for mode in ["shared", "exclusive"] {
            let mut args = base.clone();
            let at = args.iter().position(|flag| flag == "--mode").unwrap();
            args[at + 1] = mode.into();
            args.extend([
                "--start-ns".into(),
                "604800000000000".into(),
                "--end-ns".into(),
                "604800001000001".into(),
                "--preroll-ns".into(),
                "0".into(),
            ]);
            assert!(validate_args(&args).is_ok());
            let options = parse(&args).unwrap();
            for rate in [44100, 48000, 96000] {
                assert_eq!(
                    options.playback_end(rate).unwrap(),
                    Some((u64::from(rate) * 1_000_001).div_ceil(1_000_000_000))
                );
            }
            assert!(options.playback_end(0).is_err());
        }
        for end in [72_000_000_000_000i64, 604_800_000_000_000, i64::MAX] {
            let mut args = base.clone();
            args.extend(["--end-ns".into(), end.to_string()]);
            let options = parse(&args).unwrap();
            let expected = ((u128::try_from(end).unwrap() + options.preroll as u128) * 48000)
                .div_ceil(1_000_000_000);
            assert_eq!(
                options.playback_end(48000).unwrap(),
                Some(u64::try_from(expected).unwrap())
            );
        }
        for value in ["", "-1", "+1", "0", "1.5", "9223372036854775808"] {
            let mut args = base.clone();
            args.extend(["--end-ns".into(), value.into()]);
            assert!(validate_args(&args).is_err());
        }
        let mut duplicate = base.clone();
        duplicate.extend(["--end-ns".into(), "2".into(), "--end-ns".into(), "3".into()]);
        assert!(validate_args(&duplicate).is_err());
        let mut local = base.clone();
        local.extend([
            "--local-player".into(),
            "7:path-a".into(),
            "--local-player".into(),
            "4294967295:path-b".into(),
        ]);
        assert!(validate_args(&local).is_ok());
        local.extend(["--end-ns".into(), "2".into()]);
        assert!(validate_args(&local).is_ok());
        let mut network = base.clone();
        network.extend(["--mp-host".into(), "127.0.0.1:39001".into()]);
        assert!(validate_args(&network).is_ok());
        network.extend(["--end-ns".into(), "2".into()]);
        assert!(validate_args(&network).is_ok());
        local.extend(["--mp-host".into(), "127.0.0.1:39001".into()]);
        assert!(
            validate_args(&local)
                .unwrap_err()
                .to_string()
                .contains("one local participant")
        );
        let mut asio = parse(&base).unwrap();
        asio.backend = Backend::Asio;
        asio.end_ns = Some(2);
        assert!(validate_finite_modes(&asio, &Default::default()).is_ok());
    }

    #[test]
    fn finite_completion_waits_for_native_presentation_message_drain_and_resume() {
        use beatkernel::{
            audio::{AudioFormat, Mixer, MixerConfig, PcmLimits, SampleBank, command_queue},
            time::{ClockDomainId, ClockPair, ClockPoint},
        };
        use beatkernel_bms_runtime::{native_end::NativeEnd, playback_pause::NativePause};
        let host = |ns| ClockPoint {
            domain: ClockDomainId(1),
            timestamp: Timestamp::from_nanos(ns),
        };
        let output = |ns| ClockPoint {
            domain: ClockDomainId(2),
            timestamp: Timestamp::from_nanos(ns),
        };
        let pair = |ns| ClockPair {
            source: output(ns),
            target: host(ns + 100),
        };
        let format = AudioFormat::new(1000, 1).unwrap();
        let bank = SampleBank::new(format, PcmLimits::new(1024, 1024, 1).unwrap()).unwrap();
        let (mut producer, consumer) = command_queue(8).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(2),
                Timestamp::ZERO,
                AudioLimits::new(8, 1, 8, 16, 8).unwrap(),
            )
            .with_playback_end_frame(2),
            bank,
            consumer,
        )
        .unwrap();
        let mut pause = NativePause::new(output(0), ClockDomainId(1), 1000)
            .unwrap()
            .with_playback_end_frame(2)
            .unwrap();
        let mut end = NativeEnd::new(output(0), ClockDomainId(1), 1000, 2).unwrap();
        let first = mixer.render(&mut [0.0]).unwrap();
        pause.observe(Some(first), pair(0)).unwrap();
        end.observe(Some(first), pair(0)).unwrap();
        assert!(pause.request(true, pair(0)).unwrap());
        producer.request_pause(true);
        let paused = mixer.render(&mut [0.0; 3]).unwrap();
        pause
            .observe(Some(paused), pair(1_000_000))
            .unwrap()
            .unwrap();
        end.observe(Some(paused), pair(1_000_000)).unwrap();
        assert!(pause.request(false, pair(3_000_000)).unwrap());
        producer.request_pause(false);
        let crossing = mixer.render(&mut [0.0; 4]).unwrap();
        assert_eq!(crossing.playback_end_physical_frame, Some(5));
        let latest = mixer.render(&mut [0.0]).unwrap();
        let resumed = pause
            .observe(Some(latest), pair(4_000_000))
            .unwrap()
            .unwrap();
        assert_eq!(resumed.host, host(4_000_100));
        assert!(
            end.observe(Some(latest), pair(4_000_000))
                .unwrap()
                .is_none()
        );
        let song = Timestamp::from_nanos(2_000_000);
        assert!(!finite_session_done(
            Some(2_000_000),
            None,
            host(i64::MAX),
            song,
            false,
            false
        ));
        let boundary = end.observe(None, pair(6_000_000)).unwrap().unwrap();
        assert_eq!(boundary.host, host(5_000_100));
        for (watermark, backlog, resuming, logical) in [
            (host(5_000_099), false, false, song),
            (host(6_000_100), true, false, song),
            (host(6_000_100), false, true, song),
            (
                host(6_000_100),
                false,
                false,
                Timestamp::from_nanos(1_999_999),
            ),
            (output(6_000_100), false, false, song),
        ] {
            assert!(!finite_session_done(
                Some(2_000_000),
                Some(boundary.host),
                watermark,
                logical,
                backlog,
                resuming
            ));
        }
        assert!(finite_session_done(
            Some(2_000_000),
            Some(boundary.host),
            host(6_000_100),
            song,
            false,
            false
        ));
        assert!(!finite_session_done(
            None,
            Some(boundary.host),
            host(6_000_100),
            song,
            false,
            false
        ));
    }

    #[test]
    fn pending_receipts_keep_fifo_provenance_and_overflow_keeps_the_valid_prefix() {
        use beatkernel::{
            input::{
                BackendId, ButtonEvent, ButtonState, DeviceId, EventMeta, NativeEventMeta,
                PhysicalControlId, PhysicalInputEvent,
            },
            time::{ClockDomainId, ClockPoint},
        };
        let event = |time, sequence| {
            let point = ClockPoint {
                domain: ClockDomainId(1),
                timestamp: Timestamp::from_nanos(time),
            };
            let mut meta = EventMeta::new(DeviceId(7), point, sequence);
            meta.native = Some(NativeEventMeta {
                backend: BackendId(2),
                code: Some(4),
                timestamp: Some(point),
            });
            PhysicalInputEvent::Button(ButtonEvent {
                meta,
                control: PhysicalControlId::keyboard(4),
                state: ButtonState::Down,
            })
        };
        let first = event(10, 1);
        let second = event(20, 2);
        let mut inbox = std::collections::VecDeque::new();
        park_pause_input(&mut inbox, first.clone()).unwrap();
        park_pause_input(&mut inbox, second.clone()).unwrap();
        assert_eq!(inbox.pop_front(), Some(first.clone()));
        assert_eq!(inbox.pop_front(), Some(second.clone()));
        assert!(inbox.is_empty());
        inbox.resize(65536, first.clone());
        assert!(park_pause_input(&mut inbox, second).is_err());
        assert_eq!(inbox.len(), 65536);
        assert_eq!(inbox.front(), Some(&first));
        assert_eq!(inbox.back(), Some(&first));
    }
    use beatkernel::audio::{SampleId, VoiceId};
    #[test]
    fn chart_seed_is_shared_unsigned_u64_singleton_for_solo_and_local() {
        let base = arguments(None);
        assert_eq!(parse(&base).unwrap().chart_seed, 0);
        let mut local: Vec<String> = base
            .chunks_exact(2)
            .filter(|pair| pair[0] != "--keyboard-path")
            .flat_map(|pair| pair.iter().cloned())
            .collect();
        local.extend([
            "--local-player".into(),
            "3:registry:3".into(),
            "--local-player".into(),
            "4294967295:registry:4294967295".into(),
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
        let base = arguments(None);
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
    fn local_assignments_preserve_stable_ids_colons_and_lag_boundaries() {
        let mut configured = arguments(None);
        for id in [3, 9, 17, u32::MAX] {
            configured.extend(["--local-player".into(), format!("{id}:path:{id}")]);
        }
        let options = parse(&configured).unwrap();
        assert_eq!(options.local_players.len(), 4);
        assert_eq!(options.local_players[3].0.0, u32::MAX);
        assert_eq!(options.local_players[0].1, "path:3");
        assert_eq!(options.advance_lag, 2_000_000);
        for value in ["0", "1000000000"] {
            let mut args = configured.clone();
            args.extend(["--advance-lag-ns".into(), value.into()]);
            assert!(parse(&args).is_ok());
        }
        configured.extend(["--mp-host".into(), "127.0.0.1:34567".into()]);
        assert!(validate_args(&configured).is_err());
    }
    #[test]
    fn local_cli_rejects_missing_duplicate_mixed_and_oversized_assignments() {
        for values in [
            vec!["--local-player", "1:path"],
            vec!["--local-player", "0:path", "--local-player", "2:other"],
            vec!["--local-player", "+1:path", "--local-player", "2:other"],
            vec!["--local-player", "1:path", "--local-player", "1:other"],
            vec!["--local-player", "1:path", "--local-player", "2:path"],
            vec![
                "--local-player",
                "1:path",
                "--local-player",
                "2:other",
                "--keyboard-path",
                "solo",
            ],
            vec!["--advance-lag-ns", "-1"],
            vec!["--advance-lag-ns", "+1"],
            vec!["--advance-lag-ns", "1000000001"],
        ] {
            let mut configured = arguments(None);
            configured.extend(values.into_iter().map(String::from));
            assert!(parse(&configured).is_err());
        }
        let mut configured = arguments(None);
        for id in 1..=64 {
            configured.extend(["--local-player".into(), format!("{id}:path:{id}")]);
        }
        assert_eq!(parse(&configured).unwrap().local_players.len(), 64);
        configured.extend(["--local-player".into(), "65:path:65".into()]);
        assert!(parse(&configured).is_err());
    }
    #[test]
    fn settings_validation_preserves_native_and_competition_constraints() {
        let mut configured = arguments(None);
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
        let mut supplied = arguments(None);
        assert_eq!(parse(&supplied).unwrap().seconds, Some(1));
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
        let base = arguments(None);
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
        let base = arguments(None);
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
    fn arguments(preroll: Option<&str>) -> Vec<String> {
        let mut args = vec![
            "--chart",
            "fixture.bms",
            "--device",
            "explicit-id",
            "--mode",
            "shared",
            "--seconds",
            "1",
            "--bind",
            "11:04",
        ];
        if let Some(value) = preroll {
            args.extend(["--preroll-ns", value]);
        }
        args.into_iter().map(str::to_owned).collect()
    }
    #[test]
    fn explicit_preroll_cli_defaults_boundaries_and_offset_are_separate() {
        assert_eq!(parse(&arguments(None)).unwrap().preroll, 3_000_000_000);
        for (text, nanos) in [("0", 0), ("10000000000", 10_000_000_000)] {
            let options = parse(&arguments(Some(text))).unwrap();
            assert_eq!(options.preroll, nanos);
            assert_eq!(options.offset, 0);
        }
        assert!(parse(&arguments(Some("-1"))).is_err());
        assert!(parse(&arguments(Some("10000000001"))).is_err());
    }
    #[test]
    fn bgm_shift_preserves_identity_and_checks_overflow() {
        let command = AudioCommand::Play {
            voice: VoiceId(999),
            sample: SampleId(20),
            at: Timestamp::from_nanos(100),
            gain: 0.5,
        };
        assert_eq!(
            shift_bgm(command, 3_000_000_000).unwrap(),
            AudioCommand::Play {
                voice: VoiceId(999),
                sample: SampleId(20),
                at: Timestamp::from_nanos(3_000_000_100),
                gain: 0.5
            }
        );
        assert_eq!(shift_bgm(command, 0).unwrap(), command);
        assert!(
            shift_bgm(
                AudioCommand::Play {
                    at: Timestamp::from_nanos(i64::MAX),
                    voice: VoiceId(1),
                    sample: SampleId(1),
                    gain: 1.0
                },
                1
            )
            .is_err()
        );
        assert_eq!(
            calibration_extent(3600, 10_000_000_000).unwrap(),
            3_613_000_000_000
        );
        assert!(calibration_extent(u64::MAX, 0).is_err());
    }
}

#[cfg(test)]
#[path = "windows_bms/asio_fixtures.rs"]
mod asio_fixtures;
#[cfg(target_os = "windows")]
#[path = "windows_bms/output.rs"]
mod live_output;

#[cfg(target_os = "windows")]
#[path = "windows_bms/local.rs"]
mod local_native;

#[cfg(test)]
#[test]
fn exact_keyboard_selection_has_no_attachment_fallback() {
    let devices = [("path-A", 7, 11), ("path-B", 8, 12)];
    assert_eq!(selected_keyboard(None, devices).unwrap(), None);
    assert_eq!(
        selected_keyboard(Some("path-B"), devices).unwrap(),
        Some((8, 12))
    );
    assert!(selected_keyboard(Some("missing"), devices).is_err());
    assert!(selected_keyboard(Some("path-A"), [("path-A", 7, 11), ("path-A", 9, 13)]).is_err());
}
