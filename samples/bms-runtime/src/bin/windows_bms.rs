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
    exclusive: bool,
    seconds: Option<u64>,
    bindings: BTreeMap<u8, u16>,
    early: i64,
    late: i64,
    offset: i64,
    preroll: i64,
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
    let mut exclusive = None;
    let mut seconds = None;
    let mut bindings = BTreeMap::new();
    let mut key_usages = HashSet::new();
    let mut early = 150_000_000i64;
    let mut late = 150_000_000i64;
    let mut offset = 0i64;
    let mut preroll = 3_000_000_000i64;
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
        if flag != "--bind" && !seen.insert(flag.as_str()) {
            return Err(format!("duplicate option {flag}").into());
        }
        match flag.as_str() {
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
    if early < 0 || late < 0 || !(1..=AudioLimits::MAX_VOICES).contains(&voices) {
        return Err("windows must be nonnegative; voices must be 1..4096".into());
    }
    Ok(Options {
        keyboard_path,
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
    if let Some(report) = report {
        let end = report
            .start_frame
            .checked_add(u64::try_from(report.frames)?)
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
    let (_, native) = beatkernel_bms_runtime::competition_live::CompetitionOptions::extract(args)?;
    parse(&native).map(|_| ())
}

pub(crate) fn run_args(args: &[String]) -> Result<()> {
    let (competition_options, args) =
        beatkernel_bms_runtime::competition_live::CompetitionOptions::extract(args)?;
    if args.is_empty() || args == ["--help"] {
        println!(
            "windows_bms --chart PATH --device EXACT_ID [--backend wasapi|asio] --mode shared|exclusive [--seconds N] --bind channelHEX:HIDusageHEX [--bind ...]\nASIO instead requires --asio-view native|32|64 --output-channels 0,1 --asio-system-clock multimedia --asio-timer-error-ns N --asio-drift-error-ns N --asio-latency-error-ns N; optional --asio-anchor-age-ns N (default1000000000), exact --buffer frames:N or preferred default. ASIO rejects mode/period/shared-policy and ns buffers; WASAPI rejects ASIO flags. ASIO requires sample asio-sdk, SDK/MSVC toolchain and explicitly selected trusted installed driver. Error bounds are caller estimates, not physical guarantees.\nOptions: --record-replay PATH --replay-max-records N --replay-max-bytes N --bgm-lookahead-ns N --buffer default|frames:N|ns:N --period default|frames:N|ns:N --shared-policy engine|legacy --channel-policy exact|mono-stereo --voices N --early-ns N --late-ns N --input-offset-ns N --preroll-ns N\nBounds: seconds 1..3600, voices 1..4096, preroll 0..10000000000 ns, BGM lookahead positive i64 ns. Defaults: replay disabled, max records 1000000, max bytes 67108864, BGM lookahead3000000000ns, buffer/period default, shared engine, exact channels, voices256, early/late150000000ns, offset0, preroll3000000000ns. Missing --seconds plays the full song through terminal judging and reported native audio presentation; --seconds is a diagnostic loop cutoff after calibration, including remaining preroll. Bind every used BMS lane explicitly; Optional --keyboard-path EXACT_INTERFACE_PATH selects one physical keyboard; omitted accepts any physical keyboard. Explicit device removal fails the session. Focused native window. Actual supported BMS and WAV assets; no synthetic input. Physical latency unmeasured."
        );
        return Ok(());
    }
    let options = parse(&args)?;
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
        audio::{Mixer, MixerConfig, PcmLimits, command_queue},
        input::{Binding, BindingMap, DeviceSelector, GameControlId, PhysicalControlId},
        judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
        runtime::RuntimeReport,
        time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp},
        transport::Rate,
    };
    use beatkernel_bms_runtime::local_runtime::SoloRuntime as Runtime;
    use beatkernel_bms_runtime::{ChannelPolicy, load_prepared};
    use beatkernel_platform::{
        audio::{
            AudioOutputStream, AudioStreamStatus,
            presentation::{
                PresentationError, WasapiPresentationClock,
                discipline::{
                    DisciplineConfig, DisciplineError, DisciplineUpdate, ObservationAdmission,
                    PresentationDiscipline,
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
    struct ExplicitDomains;
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

    fn skippable_observation(error: &DisciplineError) -> bool {
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
    struct AcquisitionWindow {
        registration: RawInputRegistration,
        window: Option<Window>,
    }
    impl AcquisitionWindow {
        fn new() -> Result<Self> {
            let window = Window::new()?;
            let registration =
                RawInputRegistration::register(window.hwnd as usize, &[RawInputUsage::KEYBOARD])?;
            Ok(Self {
                registration,
                window: Some(window),
            })
        }
        fn hwnd(&self) -> HWND {
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
            eprintln!(
                "exact admitted-judge/failed-audio commands={:?}",
                report.audio_failures
            );
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
        let prepared = load_prepared(
            &options.chart,
            pcm,
            PcmLimits::new(64 * 1024 * 1024, 256 * 1024 * 1024, 1295)?,
            if options.mono_stereo {
                ChannelPolicy::MonoToStereo
            } else {
                ChannelPolicy::Exact
            },
        )?;
        let mut completion = beatkernel_bms_runtime::completion::SongCompletion::prepare(
            &prepared,
            options.late,
            options.offset,
            options.preroll,
            OUTPUT,
        )?;
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
        beatkernel_bms_runtime::player::publish_chart(&prepared.source, &prepared.compiled.chart)?;
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
        let mut competition = beatkernel_bms_runtime::competition_live::LiveCompetition::prepare(
            &competition_options,
            &prepared.source,
            &judge,
            HOST,
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
        let (mut producer, consumer) = command_queue(capacity)?;
        let mut bgm = BgmSession(beatkernel_bms_runtime::bgm::BgmFeeder::new(
            prepared.bgm_commands,
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
            MixerConfig::new(pcm, OUTPUT, Timestamp::ZERO, limits),
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
            "preroll={}ns; output zero maps to song -preroll; short startup pairs do not establish long-run clock stability",
            options.preroll
        );
        if options.preroll == 0 {
            println!(
                "zero preroll: calibration can consume initial BGM/notes before the gameplay pump"
            );
        }
        let mut capture = None;
        let mut pre_origin_inputs = 0u64;
        let outcome = (|| -> Result<()> {
            if options.record_replay.is_some() {
                let limits = beatkernel::replay::codec::ReplayCodecLimits::new(
                    options.replay_max_bytes,
                    options.replay_max_records,
                    4096,
                    beatkernel::input::CodecLimits::new(65536, 32768)?,
                )?;
                capture = Some(
                    beatkernel_bms_runtime::replay_capture::LiveReplayCapture::new(
                        &judge, HOST, limits,
                    )?,
                );
            }
            stream.start()?;
            let (mut transport, quality) = stream.calibrate(
                &options,
                calibration_extent(
                    options.seconds.unwrap_or(completion.calibration_seconds()),
                    options.preroll,
                )?,
                &mut bgm,
                &mut producer,
            )?;
            transport.set_rate(transport.anchor().host_time, Rate::NORMAL)?;
            let mut discipline = PresentationDiscipline::new(
                DisciplineConfig::default(),
                ClockPoint {
                    domain: OUTPUT,
                    timestamp: Timestamp::ZERO,
                },
                HOST,
                Timestamp::from_nanos(-options.preroll),
            )?;
            stream.seed(&mut discipline, &mut bgm, &mut producer)?;
            discipline.validate_host(clock.sample()?.normalized)?;
            println!(
                "presentation discipline seed={:?} config={:?} quality={:?}; ongoing continuous transport correction, PCM/BGM rate unchanged",
                discipline.latest_pair(),
                discipline.config(),
                discipline.quality()
            );
            println!(
                "observed output-zero/song-minus-preroll anchor={:?}; mapping quality={:?}; keysound scheduling=backend software output frontier/Unknown; physical latency=unmeasured",
                transport.anchor(),
                quality
            );
            let initial_host = transport.anchor().host_time;
            let mut last_accepted_host = initial_host;
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
            let mut last_progress_second = None;
            'pump: while deadline.is_none_or(|deadline| Instant::now() < deadline)
                && !beatkernel_bms_runtime::player::cancelled()
            {
                // Missing/degraded readings can skip only while real progressing
                // observations stay fresh. Terminal/native chronology errors stop.
                let _admission = stream.observe(&mut discipline)?;
                feed_rendered(&mut bgm, stream.render_report()?, |command| {
                    runtime.enqueue_audio(command)
                })?;
                discipline.validate_host(clock.sample()?.normalized)?;
                // SAFETY: MSG is an initialized POD native message buffer, local to this thread.
                let mut message: MSG = unsafe { std::mem::zeroed() };
                let mut processed_messages = 0;
                // SAFETY: writable MSG local to the owning native message thread.
                while processed_messages < 256
                    && unsafe { PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) } != 0
                {
                    processed_messages += 1;
                    if deadline.is_some_and(|deadline| Instant::now() >= deadline)
                        || message.message == WM_QUIT
                        || message.message == WM_CLOSE
                    {
                        break 'pump;
                    }
                    if message.hwnd == acquisition.hwnd() && message.message == WM_INPUT {
                        let acquired =
                            input.read_raw_input(message.lParam as usize, Some(message.time));
                        if message.wParam & 0xff == 0 {
                            // SAFETY: actual foreground Raw Input message, cleaned once even on decode failure.
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
                            let host = ClockPoint {
                                domain: event.meta().clock_domain,
                                timestamp: event.meta().timestamp,
                            };
                            // Raw Input metadata is QPC receipt time; this fresh point
                            // measures software delivery, not native hardware age.
                            let received = clock.sample()?.normalized;
                            if options.backend == Backend::Asio {
                                if host.timestamp > received.timestamp {
                                    return Err(
                                        "ASIO live input is later than fresh QPC receipt".into()
                                    );
                                }
                                if host.timestamp < initial_host {
                                    pre_origin_inputs = pre_origin_inputs
                                        .checked_add(1)
                                        .ok_or("pre-origin input counter overflow")?;
                                    continue;
                                }
                                if host.timestamp < last_accepted_host {
                                    return Err("ASIO live input host chronology regressed".into());
                                }
                            }
                            discipline.validate_host(received)?;
                            discipline.validate_host(host)?;
                            delivery.observe(host, received)?;
                            last_accepted_host = host.timestamp;
                            print_report(
                                runtime.process_input(
                                    event,
                                    &ExplicitDomains,
                                    stream.schedule(pcm.sample_rate())?,
                                )?,
                                &mut capture,
                                &mut competition,
                            )?;
                        }
                        continue;
                    }
                    if message.hwnd == acquisition.hwnd()
                        && message.message == WM_INPUT_DEVICE_CHANGE
                    {
                        match message.wParam as u32 {
                            GIDC_ARRIVAL => {
                                input.attach_device(message.lParam as usize)?;
                            }
                            GIDC_REMOVAL => {
                                if selected
                                    .is_some_and(|(_, handle)| handle == message.lParam as usize)
                                {
                                    return Err("selected keyboard detached; restart with an explicit attached device".into());
                                }
                                input.remove_device(message.lParam as usize);
                            }
                            _ => {}
                        }
                    }
                    // SAFETY: real initialized message, stateless owning-window procedure.
                    unsafe {
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
                let host = clock.sample()?.normalized;
                if options.backend == Backend::Asio && host.timestamp < initial_host {
                    std::thread::sleep(WallDuration::from_millis(1));
                    continue;
                }
                discipline.validate_host(host)?;
                if let DisciplineUpdate::Applied {
                    base_rate_ppm,
                    correction_ppm,
                    applied_rate_ppm,
                    phase_error_ns,
                    limited,
                } = discipline.update(host, runtime.transport_mut())?
                {
                    println!(
                        "presentation discipline measured={base_rate_ppm:+}ppm correction={correction_ppm:+}ppm applied={applied_rate_ppm:+}ppm phase={phase_error_ns}ns limited={limited} quality={:?}",
                        discipline.quality()
                    );
                }
                let report = runtime.advance_to(
                    host,
                    &ExplicitDomains,
                    stream.schedule(pcm.sample_rate())?,
                )?;
                last_accepted_host = host.timestamp;
                let last_song = report.song_time;
                let nanos = last_song.as_nanos();
                let second = nanos.div_euclid(1_000_000_000);
                if last_progress_second != Some(second) {
                    if nanos < 0 {
                        let remaining = (-i128::from(nanos) + 999_999_999) / 1_000_000_000;
                        println!(
                            "song countdown={remaining}s, logical song={nanos}ns; focus native window"
                        );
                    } else {
                        println!("logical song={nanos}ns; focus native window");
                    }
                    last_progress_second = Some(second);
                }
                print_report(report, &mut capture, &mut competition)?;
                if completion.observe(
                    runtime.judge(),
                    last_song,
                    bgm.report(),
                    stream.render_report()?,
                    discipline.latest_pair().map(|pair| pair.source),
                )? {
                    println!(
                        "full song completed: terminal judge, drained BGM/mixer and native presentation frontier"
                    );
                    break;
                }
                std::thread::sleep(WallDuration::from_millis(1));
            }
            println!(
                "runtime counters={:?} software processing={:?}",
                runtime.telemetry().counters(),
                runtime.telemetry().processing()
            );
            Ok(())
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
    use beatkernel::audio::{SampleId, VoiceId};
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
