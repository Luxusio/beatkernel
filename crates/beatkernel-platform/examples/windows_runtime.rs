//! Native Raw Input -> binding -> judging -> WASAPI composition.
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["--help"] {
        println!(
            "BeatKernel Windows integrated runtime\nUsage: windows_runtime --device ID --mode shared|exclusive [--seconds 1..60] [--song FILE.wav --start-ns N] [--restarts 1..8]\nFour canonical keys: D F J K. Each restart opens fresh output. Explicit endpoint/mode; source WAV channels must match native mix channels.\nUses observed presentation/QPC pairs for song origin; measurement uncertainty and physical latency remain unknown."
        );
        return Ok(());
    }
    let mut device = None;
    let mut exclusive = None;
    let mut seconds = 10u64;
    let mut song = None;
    let mut start_nanos = 0i64;
    let mut restarts = 1u32;
    let mut args = args.iter();
    while let Some(argument) = args.next() {
        let value = args.next().ok_or("option requires a value")?;
        match argument.as_str() {
            "--device" if !value.is_empty() => device = Some(value.clone()),
            "--mode" => {
                exclusive = Some(match value.as_str() {
                    "shared" => false,
                    "exclusive" => true,
                    _ => return Err("mode must be shared or exclusive".into()),
                })
            }
            "--seconds" => {
                seconds = value.parse()?;
                if !(1..=60).contains(&seconds) {
                    return Err("seconds must be 1..60".into());
                }
            }
            "--song" if !value.is_empty() => song = Some(value.clone()),
            "--start-ns" => start_nanos = value.parse()?,
            "--restarts" => {
                restarts = value.parse()?;
                if !(1..=8).contains(&restarts) {
                    return Err("restarts must be 1..8".into());
                }
            }
            _ => return Err("unknown option".into()),
        }
    }
    let device = device.ok_or("explicit --device required")?;
    let exclusive = exclusive.ok_or("explicit --mode required")?;
    if start_nanos < 0 || (start_nanos != 0 && song.is_none()) {
        return Err("nonnegative --start-ns requires --song".into());
    }
    #[cfg(target_os = "windows")]
    {
        native::run(device, exclusive, seconds, song, start_nanos, restarts)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (device, exclusive, seconds, song, start_nanos, restarts);
        Err("native integrated runtime requires Windows".into())
    }
}

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use beatkernel::{
        audio::{
            command_queue, AudioLimits, Mixer, MixerConfig, PcmLimits, PcmSample, SampleBank,
            SampleId, VoiceId,
        },
        chart::{
            Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
        },
        input::{Binding, BindingMap, DeviceSelector, GameControlId, PhysicalControlId},
        interaction::InstantEvaluator,
        judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow, Rule},
        runtime::restart::{FrameRounding, RestartPlan},
        runtime::{Runtime, SoundBinding},
        time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    };
    use beatkernel_platform::{
        audio::presentation::{PresentationError, WasapiPresentationClock},
        audio::{
            AudioBackendKind, AudioDeviceId, AudioOutputBackend, AudioOutputStream,
            AudioStreamMode, AudioStreamRequest, AudioStreamStatus, BufferRequest, PeriodRequest,
        },
        windows::{
            audio::{WasapiBackend, WasapiOptions, WasapiStream},
            clock::QpcClock,
            input::{RawInputRegistration, RawInputUsage, WindowsInput},
        },
    };
    use std::{
        fs::File,
        io,
        io::Read,
        ptr,
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
        fn new() -> Result<Self, Box<dyn Error>> {
            let class: Vec<u16> = format!("BeatKernelRuntime{}", std::process::id())
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
            let title: Vec<u16> = "BeatKernel four-key runtime: D F J K"
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

    fn schedule_at(stream: &WasapiStream, rate: u32) -> Result<ClockPoint, Box<dyn Error>> {
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

    fn presentation(
        stream: &WasapiStream,
        seconds: u64,
    ) -> Result<WasapiPresentationClock, Box<dyn Error>> {
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
                            end: Timestamp::from_nanos(i64::try_from(seconds + 2)? * 1_000_000_000),
                        },
                        ExtrapolationPolicy::Bounded {
                            before: Duration::from_nanos(2_000_000_000),
                            after: Duration::from_nanos(
                                i64::try_from(seconds + 2)? * 1_000_000_000,
                            ),
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

    pub(super) fn run(
        device: String,
        exclusive: bool,
        seconds: u64,
        song: Option<String>,
        start_nanos: i64,
        restarts: u32,
    ) -> Result<(), Box<dyn Error>> {
        let source = if let Some(path) = song {
            let limits = PcmLimits::new(256 * 1024 * 1024, 512 * 1024 * 1024, 5)?;
            let mut bytes = Vec::new();
            File::open(path)?
                .take(256 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 256 * 1024 * 1024 {
                return Err("WAV exceeds 256 MiB example limit".into());
            }
            Some(PcmSample::from_wav(&bytes, limits)?)
        } else {
            None
        };
        for iteration in 0..restarts {
            println!(
                "restart {} of {restarts}: fresh stream/queue",
                iteration + 1
            );
            if !run_once(
                device.clone(),
                exclusive,
                seconds,
                source.as_ref(),
                start_nanos,
            )? {
                break;
            }
        }
        Ok(())
    }

    fn run_once(
        device: String,
        exclusive: bool,
        seconds: u64,
        source: Option<&PcmSample>,
        start_nanos: i64,
    ) -> Result<bool, Box<dyn Error>> {
        let clock = QpcClock::new(HOST)?;
        let backend = WasapiBackend;
        let device = AudioDeviceId(device);
        let format = backend.mix_format(&device)?;
        let request = AudioStreamRequest::new(
            device,
            AudioBackendKind::Wasapi,
            if exclusive {
                AudioStreamMode::Exclusive
            } else {
                AudioStreamMode::Shared(
                    beatkernel_platform::audio::SharedPeriodPolicy::EnginePeriod,
                )
            },
            format,
            BufferRequest::DeviceDefault,
            PeriodRequest::DeviceDefault,
        )?;
        let pcm = format.pcm();
        let (mut producer, consumer) = command_queue(256)?;
        let pcm_limits = PcmLimits::new(256 * 1024 * 1024, 512 * 1024 * 1024, 5)?;
        let mut bank = SampleBank::new(pcm, pcm_limits)?;
        let mut applied_song = Timestamp::ZERO;
        if let Some(source) = source {
            let selected = RestartPlan::select(
                source,
                Timestamp::ZERO,
                Timestamp::from_nanos(start_nanos),
                FrameRounding::Nearest,
            )?;
            applied_song = selected.applied_song_time();
            bank.insert(SampleId(0), selected.copy_pcm(pcm_limits)?)?;
            producer
                .try_push(beatkernel::audio::AudioCommand::Play {
                    sample: SampleId(0),
                    voice: VoiceId(0),
                    gain: 1.0,
                    at: Timestamp::ZERO,
                })
                .map_err(|error| format!("music admission: {error:?}"))?;
            println!(
                "source frame={} requested={}ns applied={}ns correction={}ns",
                selected.source_frame(),
                start_nanos,
                applied_song.as_nanos(),
                selected.correction_nanos()
            );
        }
        for lane in 0..4 {
            let frames = (pcm.sample_rate() / 10) as usize;
            let mut samples = Vec::with_capacity(frames * usize::from(pcm.channels()));
            for frame in 0..frames {
                let value = (frame as f32 * (440.0 + lane as f32 * 110.0) * std::f32::consts::TAU
                    / pcm.sample_rate() as f32)
                    .sin()
                    * 0.15;
                samples.extend(std::iter::repeat_n(value, usize::from(pcm.channels())));
            }
            bank.insert(
                SampleId(lane + 1),
                PcmSample::new(pcm, samples, pcm_limits)?,
            )?;
        }
        let mixer = Mixer::new(
            MixerConfig::new(
                pcm,
                OUTPUT,
                Timestamp::ZERO,
                AudioLimits::new(256, 64, 256, 192_000, 256)?,
            ),
            bank,
            consumer,
        )?;
        let mut stream = backend.open(request, mixer, clock, WasapiOptions::default())?;
        let mut chart = SourceChart::new(1_000_000_000, Bpm::new(60, 1)?)?;
        let mut sounds = Vec::new();
        for second in 1..=seconds {
            for lane in 0..4u32 {
                let id = ObjectId(second * 4 + u64::from(lane));
                chart.objects.push(SourceObject {
                    id,
                    start: Beat::new(
                        applied_song
                            .as_nanos()
                            .checked_add(i64::try_from(second)? * 1_000_000_000)
                            .ok_or("chart cue overflow")?,
                    )?,
                    end: None,
                    interaction: InteractionId(lane),
                    visual: VisualId(lane),
                    audio: None,
                    metadata: ObjectMetadata::default(),
                });
                sounds.push(SoundBinding {
                    object: id,
                    stage: JudgeStage::Instant,
                    sample: SampleId(u64::from(lane + 1)),
                    voice: VoiceId(u64::from(lane + 1)),
                    gain: 1.0,
                });
            }
        }
        let rules = (0..4)
            .map(|lane| Rule {
                interaction: InteractionId(lane),
                control: GameControlId(lane),
                evaluator: Box::new(InstantEvaluator),
            })
            .collect();
        let judge = JudgeEngine::new(
            chart.compile()?,
            rules,
            JudgeProfile::new(
                vec![JudgeWindow {
                    grade: JudgeGrade(1),
                    early: Duration::from_nanos(150_000_000),
                    late: Duration::from_nanos(150_000_000),
                }],
                Duration::ZERO,
            )?,
        )?;
        let bindings = BindingMap::from_bindings([7, 9, 13, 14].into_iter().enumerate().map(
            |(lane, key)| Binding {
                device: DeviceSelector::Any,
                physical: PhysicalControlId::keyboard(key),
                game_control: GameControlId(lane as u32),
            },
        ))?;
        let window = Window::new()?;
        let mut registration =
            RawInputRegistration::register(window.hwnd as usize, &[RawInputUsage::KEYBOARD])?;
        let mut input = WindowsInput::new(clock);
        input.enumerate_devices()?;
        stream.start()?;
        let (relation, transport) = match presentation(&stream, seconds).and_then(|relation| {
            let transport = relation.transport(applied_song)?;
            Ok((relation, transport))
        }) {
            Ok(pair) => pair,
            Err(error) => {
                let close = registration.close();
                let stop = stream.stop();
                close?;
                stop?;
                return Err(error);
            }
        };
        println!(
            "observed song anchor={:?} mapping_quality={:?}; physical accuracy remains unmeasured",
            transport.anchor(),
            relation.quality()
        );
        let mut runtime = Runtime::new(
            HOST, OUTPUT, transport, bindings, judge, producer, sounds, 4096,
        )?;
        println!(
            "requested/applied={:?}; keys D F J K target each whole song second",
            stream.configuration()
        );
        let deadline = Instant::now() + WallDuration::from_secs(seconds);
        let mut closed = false;
        let outcome = (|| -> Result<(), Box<dyn Error>> {
            'pump: while Instant::now() < deadline {
                let mut message: MSG = unsafe { std::mem::zeroed() };
                // SAFETY: writable MSG belongs to this thread; native queue fills it synchronously.
                while unsafe { PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
                    if message.message == WM_QUIT || message.message == WM_CLOSE {
                        closed = true;
                        break 'pump;
                    }
                    if message.hwnd == window.hwnd && message.message == WM_INPUT {
                        let acquired =
                            input.read_raw_input(message.lParam as usize, Some(message.time));
                        if message.wParam & 0xff == 0 {
                            // SAFETY: actual foreground Raw Input message, cleaned once after acquisition.
                            unsafe {
                                DefWindowProcW(
                                    message.hwnd,
                                    message.message,
                                    message.wParam,
                                    message.lParam,
                                );
                            }
                        }
                        let batch = acquired?;
                        for event in batch.input.events {
                            relation.mapper().map_checked(
                                ClockPoint {
                                    domain: event.meta().clock_domain,
                                    timestamp: event.meta().timestamp,
                                },
                                OUTPUT,
                            )?;
                            let report = runtime.process_input(
                                event,
                                &ExplicitDomains,
                                schedule_at(&stream, pcm.sample_rate())?,
                            )?;
                            for result in report.judge_events {
                                println!("judge={result:?}");
                            }
                            if report.judge_error.is_some() || !report.audio_failures.is_empty() {
                                eprintln!(
                                    "judge_error={:?} queue_failures={:?}",
                                    report.judge_error, report.audio_failures
                                );
                            }
                        }
                        continue;
                    }
                    if message.hwnd == window.hwnd && message.message == WM_INPUT_DEVICE_CHANGE {
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
                    // SAFETY: initialized actual message, stateless no-unwind window procedure.
                    unsafe {
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
                let host = clock.sample()?.normalized;
                relation.mapper().map_checked(host, OUTPUT)?;
                let report = runtime.advance_to(
                    host,
                    &ExplicitDomains,
                    schedule_at(&stream, pcm.sample_rate())?,
                )?;
                for result in report.judge_events {
                    println!("judge={result:?}");
                }
                std::thread::sleep(WallDuration::from_millis(1));
            }
            Ok(())
        })();
        let registration_result = registration.close();
        let stop_result = stream.stop();
        println!(
            "software_processing={:?} runtime_counters={:?} audio_snapshot={:?}; physical_latency=unmeasured",
            runtime.telemetry().processing(),
            runtime.telemetry().counters(),
            stream.snapshot()
        );
        outcome?;
        registration_result?;
        stop_result?;
        Ok(!closed)
    }
}
