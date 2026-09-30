//! Real BMS/WAV assets, physical keyboard acquisition and explicit native WASAPI.
use beatkernel::{
    audio::{AudioCommand, AudioLimits},
    time::{Duration, Timestamp},
};
use beatkernel_platform::audio::{BufferRequest, PeriodRequest, SharedPeriodPolicy};
use std::{
    collections::{BTreeMap, HashSet},
    error::Error,
    path::PathBuf,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
struct Options {
    chart: PathBuf,
    device: String,
    exclusive: bool,
    seconds: u64,
    bindings: BTreeMap<u8, u16>,
    early: i64,
    late: i64,
    offset: i64,
    preroll: i64,
    voices: usize,
    mono_stereo: bool,
    buffer: BufferRequest,
    period: PeriodRequest,
    shared: SharedPeriodPolicy,
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
    let mut chart = None;
    let mut device = None;
    let mut exclusive = None;
    let mut seconds = None;
    let mut bindings = BTreeMap::new();
    let mut key_usages = HashSet::new();
    let mut early = 150_000_000i64;
    let mut late = 150_000_000i64;
    let mut offset = 0i64;
    let mut preroll = 3_000_000_000i64;
    let mut voices = 256usize;
    let mut mono_stereo = false;
    let mut buffer = BufferRequest::DeviceDefault;
    let mut period = PeriodRequest::DeviceDefault;
    let mut shared = SharedPeriodPolicy::EnginePeriod;
    let mut seen = HashSet::new();
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        let value = iter.next().ok_or("each option requires a value")?;
        if flag != "--bind" && !seen.insert(flag.as_str()) {
            return Err(format!("duplicate option {flag}").into());
        }
        match flag.as_str() {
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
    let exclusive = exclusive.ok_or("explicit --mode required")?;
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
        chart: chart.ok_or("explicit --chart required")?,
        device: device.ok_or("explicit --device required")?,
        exclusive,
        seconds: seconds.ok_or("explicit --seconds required")?,
        bindings,
        early,
        late,
        offset,
        preroll,
        voices,
        mono_stereo,
        buffer,
        period,
        shared,
    })
}
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn shift_bgm(command: AudioCommand, preroll: i64) -> Result<AudioCommand> {
    if !(0..=10_000_000_000).contains(&preroll) {
        return Err("invalid preroll".into());
    }
    match command {
        AudioCommand::Play {
            voice,
            sample,
            at,
            gain,
        } => Ok(AudioCommand::Play {
            voice,
            sample,
            at: Timestamp::from_nanos(
                at.as_nanos()
                    .checked_add(preroll)
                    .ok_or("BGM/preroll timestamp overflow")?,
            ),
            gain,
        }),
        _ => Err("prepared BGM must contain only Play commands".into()),
    }
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

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["--help"] {
        println!("windows_bms --chart PATH --device EXACT_ID --mode shared|exclusive --seconds 1..3600 --bind channelHEX:HIDusageHEX [--bind ...]\nOptions: --buffer default|frames:N|ns:N --period default|frames:N|ns:N --shared-policy engine|legacy --channel-policy exact|mono-stereo --voices 1..4096 --early-ns N --late-ns N --input-offset-ns N --preroll-ns 0..10000000000\nDefaults: buffer/period default, shared engine, exact channels, voices256, early/late150000000ns, offset0, preroll3000000000ns. Seconds is loop duration after calibration, including remaining preroll. Bind every used BMS lane explicitly; Any physical keyboard, focused native window. Actual supported BMS and WAV assets; no synthetic input. Physical latency unmeasured.");
        return Ok(());
    }
    let options = parse(&args)?;
    #[cfg(target_os = "windows")]
    {
        native::run(options)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = options;
        Err("windows_bms native playback requires Windows".into())
    }
}

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use beatkernel::{
        audio::{command_queue, Mixer, MixerConfig, PcmLimits},
        input::{Binding, BindingMap, DeviceSelector, GameControlId, PhysicalControlId},
        judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
        runtime::{Runtime, RuntimeReport},
        time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp},
        transport::Rate,
    };
    use beatkernel_bms_runtime::{load_prepared, ChannelPolicy};
    use beatkernel_platform::{
        audio::{
            presentation::{
                discipline::{
                    DisciplineConfig, DisciplineError, DisciplineUpdate, ObservationAdmission,
                    PresentationDiscipline,
                },
                PresentationError, WasapiPresentationClock,
            },
            AudioBackendKind, AudioDeviceId, AudioOutputBackend, AudioOutputStream,
            AudioStreamMode, AudioStreamRequest, AudioStreamStatus,
        },
        windows::{
            audio::{WasapiBackend, WasapiOptions, WasapiStream},
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
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW,
            RegisterClassW, TranslateMessage, UnregisterClassW, GIDC_ARRIVAL, GIDC_REMOVAL, MSG,
            PM_REMOVE, WM_CLOSE, WM_INPUT, WM_INPUT_DEVICE_CHANGE, WM_QUIT, WNDCLASSW,
            WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        },
    };
    const HOST: ClockDomainId = ClockDomainId(1);
    const OUTPUT: ClockDomainId = ClockDomainId(2);
    struct ExplicitDomains;
    impl ClockMapper for ExplicitDomains {
        fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
            None
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Unknown
        }
    }
    struct Window {
        hwnd: HWND,
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
            let class: Vec<u16> = format!("BeatKernelBms{}", std::process::id())
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
                    WS_OVERLAPPEDWINDOW | WS_VISIBLE,
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

    fn schedule_at(stream: &WasapiStream, rate: u32) -> Result<ClockPoint> {
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

    fn presentation(stream: &WasapiStream, extent: i64) -> Result<WasapiPresentationClock> {
        use beatkernel::time::{CalibrationUncertainty, ClockInterval, ExtrapolationPolicy};
        use beatkernel_platform::audio::AudioClockReadingQuality;
        let deadline = Instant::now() + WallDuration::from_secs(2);
        let mut first = None;
        while Instant::now() < deadline {
            let snapshot = stream.snapshot();
            if snapshot.status != AudioStreamStatus::Running {
                return Err("audio terminated before presentation calibration".into());
            }
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
    fn observe_running(
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
    fn seed_discipline(
        stream: &WasapiStream,
        discipline: &mut PresentationDiscipline,
    ) -> Result<()> {
        let deadline = Instant::now() + WallDuration::from_secs(2);
        while Instant::now() < deadline {
            if matches!(
                observe_running(stream, discipline)?,
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
    fn print_report(report: RuntimeReport) -> Result<()> {
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
    pub(super) fn run(options: Options) -> Result<()> {
        let clock = QpcClock::new(HOST)?;
        let backend = WasapiBackend;
        let device = AudioDeviceId(options.device.clone());
        let format = backend.mix_format(&device)?;
        let request = AudioStreamRequest::new(
            device,
            AudioBackendKind::Wasapi,
            if options.exclusive {
                AudioStreamMode::Exclusive
            } else {
                AudioStreamMode::Shared(options.shared)
            },
            format,
            options.buffer,
            options.period,
        )?;
        let pcm = format.pcm();
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
        let bindings =
            BindingMap::from_bindings(options.bindings.iter().map(|(&channel, &usage)| Binding {
                device: DeviceSelector::Any,
                physical: PhysicalControlId::keyboard(usage),
                game_control: GameControlId(u32::from(channel)),
            }))?;
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
        const LIVE_SLACK: usize = 1024;
        let capacity = prepared
            .bgm_commands
            .len()
            .checked_add(LIVE_SLACK)
            .ok_or("BGM capacity overflow")?;
        if capacity > AudioLimits::MAX_COMMANDS {
            return Err("BGM count exceeds 64512, leaving 1024 reserved live command slots".into());
        }
        let limits = AudioLimits::new(
            capacity,
            options.voices,
            capacity,
            AudioLimits::MAX_RENDER_FRAMES,
            capacity,
        )?;
        let (mut producer, consumer) = command_queue(capacity)?;
        for command in prepared.bgm_commands {
            // Output zero corresponds to song -preroll; compiled targets stay unchanged.
            producer
                .try_push(shift_bgm(command, options.preroll)?)
                .map_err(|error| {
                    format!("BGM admission failed, exact command/reason: {error:?}")
                })?;
        }
        let mixer = Mixer::new(
            MixerConfig::new(pcm, OUTPUT, Timestamp::ZERO, limits),
            prepared.bank,
            consumer,
        )?;
        println!("explicit Any-keyboard bindings={:?}; windows early={}ns late={}ns offset={}ns; channel_policy={} active_voices={} queue/pending={} reserved_live={LIVE_SLACK}",
            options.bindings, options.early, options.late, options.offset,
            if options.mono_stereo { "mono-stereo" } else { "exact" }, options.voices, capacity);
        let mut acquisition = AcquisitionWindow::new()?;
        let mut input = WindowsInput::new(clock);
        input.enumerate_devices()?;
        let mut stream = backend.open(request, mixer, clock, WasapiOptions::default())?;
        println!(
            "requested/applied native output={:?}",
            stream.configuration()
        );
        println!("Focus the BeatKernel BMS native window and play the explicitly bound physical keys. Console prints actual grades and misses.");
        println!("preroll={}ns; output zero maps to song -preroll; short startup pairs do not establish long-run clock stability", options.preroll);
        if options.preroll == 0 {
            println!(
                "zero preroll: calibration can consume initial BGM/notes before the gameplay pump"
            );
        }
        let outcome = (|| -> Result<()> {
            stream.start()?;
            let relation = presentation(
                &stream,
                calibration_extent(options.seconds, options.preroll)?,
            )?;
            let mut transport = relation.transport(Timestamp::from_nanos(-options.preroll))?;
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
            seed_discipline(&stream, &mut discipline)?;
            discipline.validate_host(clock.sample()?.normalized)?;
            println!("presentation discipline seed={:?} config={:?} quality={:?}; ongoing continuous transport correction, PCM/BGM rate unchanged", discipline.latest_pair(), discipline.config(), discipline.quality());
            println!("observed output-zero/song-minus-preroll anchor={:?}; mapping quality={:?}; keysound scheduling=submitted frame grid/Unknown; physical latency=unmeasured", transport.anchor(), relation.quality());
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
            let mut last_progress_second = None;
            'pump: while Instant::now() < deadline {
                // Missing/degraded readings can skip only while real progressing
                // observations stay fresh. Terminal/native chronology errors stop.
                let _admission = observe_running(&stream, &mut discipline)?;
                discipline.validate_host(clock.sample()?.normalized)?;
                // SAFETY: MSG is an initialized POD native message buffer, local to this thread.
                let mut message: MSG = unsafe { std::mem::zeroed() };
                let mut processed_messages = 0;
                // SAFETY: writable MSG local to the owning native message thread.
                while processed_messages < 256
                    && unsafe { PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) } != 0
                {
                    processed_messages += 1;
                    if Instant::now() >= deadline
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
                            discipline.validate_host(ClockPoint {
                                domain: event.meta().clock_domain,
                                timestamp: event.meta().timestamp,
                            })?;
                            print_report(runtime.process_input(
                                event,
                                &ExplicitDomains,
                                schedule_at(&stream, pcm.sample_rate())?,
                            )?)?;
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
                discipline.validate_host(host)?;
                if let DisciplineUpdate::Applied {
                    base_rate_ppm,
                    correction_ppm,
                    applied_rate_ppm,
                    phase_error_ns,
                    limited,
                } = discipline.update(host, runtime.transport_mut())?
                {
                    println!("presentation discipline measured={base_rate_ppm:+}ppm correction={correction_ppm:+}ppm applied={applied_rate_ppm:+}ppm phase={phase_error_ns}ns limited={limited} quality={:?}", discipline.quality());
                }
                let report = runtime.advance_to(
                    host,
                    &ExplicitDomains,
                    schedule_at(&stream, pcm.sample_rate())?,
                )?;
                let nanos = report.song_time.as_nanos();
                let second = nanos.div_euclid(1_000_000_000);
                if last_progress_second != Some(second) {
                    if nanos < 0 {
                        let remaining = (-i128::from(nanos) + 999_999_999) / 1_000_000_000;
                        println!("song countdown={remaining}s, logical song={nanos}ns; focus native window");
                    } else {
                        println!("logical song={nanos}ns; focus native window");
                    }
                    last_progress_second = Some(second);
                }
                print_report(report)?;
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
        let stop = stream.stop(); // joins worker and frees native COM output there
        let close = acquisition.registration.close();
        println!(
            "final audio snapshot={:?}; physical latency=unmeasured",
            stream.snapshot()
        );
        if let Err(error) = &stop {
            eprintln!("native output stop/join error: {error}");
        }
        if let Err(error) = &close {
            eprintln!("Raw Input unregister error: {error}");
        }
        outcome?;
        stop?;
        close?;
        Ok(())
    }
}

#[cfg(test)]
mod preroll_fixtures {
    use super::*;
    use beatkernel::audio::{SampleId, VoiceId};
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
        assert!(shift_bgm(
            AudioCommand::Play {
                at: Timestamp::from_nanos(i64::MAX),
                voice: VoiceId(1),
                sample: SampleId(1),
                gain: 1.0
            },
            1
        )
        .is_err());
        assert_eq!(
            calibration_extent(3600, 10_000_000_000).unwrap(),
            3_613_000_000_000
        );
        assert!(calibration_extent(u64::MAX, 0).is_err());
    }
}
