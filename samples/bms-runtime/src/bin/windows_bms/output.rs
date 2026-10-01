//! Control-thread backend ownership shared by the physical-input gameplay pump.
#[cfg(feature = "asio-sdk")]
use super::native::OUTPUT;
#[cfg(feature = "asio-sdk")]
use super::native::Window;
use super::*;
#[cfg(feature = "asio-sdk")]
use beatkernel::time::Timestamp;
use beatkernel::{
    audio::{AudioFormat, CommandProducer, Mixer, RenderReport},
    time::{ClockMappingQuality, ClockPoint},
    transport::Transport,
};
#[cfg(feature = "asio-sdk")]
use beatkernel_platform::{
    audio::asio::{
        AsioBufferRequest, AsioPresentationClock, AsioPresentationError,
        AsioPresentationObservation, MultimediaClockAnchor,
    },
    windows::asio::{
        AsioEnumerationLimits, AsioRegistryView,
        control::AsioControl,
        enumerate_asio_drivers,
        stream::{AsioStream, AsioStreamPhase},
    },
};
use beatkernel_platform::{
    audio::{
        AudioBackendKind, AudioDeviceId, AudioOutputBackend, AudioOutputStream, AudioStreamMode,
        AudioStreamRequest,
        presentation::discipline::{ObservationAdmission, PresentationDiscipline},
    },
    windows::{
        audio::{WasapiBackend, WasapiOptions, WasapiStream},
        clock::QpcClock,
    },
};
#[cfg(feature = "asio-sdk")]
use std::time::{Duration as WallDuration, Instant};

pub(super) enum Setup {
    Wasapi(AudioStreamRequest),
    #[cfg(feature = "asio-sdk")]
    Asio {
        control: AsioControl,
        window: Window,
        format: AudioFormat,
    },
}
impl Setup {
    pub(super) fn new(options: &Options, _clock: QpcClock) -> Result<Self> {
        if options.backend == Backend::Wasapi {
            let device = AudioDeviceId(options.device.clone());
            let format = WasapiBackend.mix_format(&device)?;
            return Ok(Self::Wasapi(AudioStreamRequest::new(
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
            )?));
        }
        #[cfg(not(feature = "asio-sdk"))]
        return Err("ASIO requires sample feature asio-sdk".into());
        #[cfg(feature = "asio-sdk")]
        {
            let view = match options.asio_view.ok_or("ASIO view required")? {
                AsioView::Native => AsioRegistryView::Native,
                AsioView::Bits32 => AsioRegistryView::Bits32,
                AsioView::Bits64 => AsioRegistryView::Bits64,
            };
            let drivers = enumerate_asio_drivers(view, AsioEnumerationLimits::default())?;
            let mut matches = drivers.iter().filter(|d| d.id.clsid == options.device);
            let driver = matches
                .next()
                .ok_or("selected ASIO CLSID is absent in explicit registry view")?;
            if matches.next().is_some() {
                return Err("ambiguous ASIO registration".into());
            }
            let window = Window::hidden()?;
            // SAFETY: selected installed driver is explicitly trusted. Hidden
            // owner-thread HWND is retained through control/stream Release.
            let mut control = unsafe { AsioControl::open(driver, Some(window.hwnd as usize)) }?;
            let rate = control.sample_rate()?;
            if !rate.is_finite() || rate <= 0.0 || rate.fract() != 0.0 || rate > f64::from(u32::MAX)
            {
                return Err("ASIO rate must be integral positive u32".into());
            }
            let channels = options
                .output_channels
                .as_ref()
                .ok_or("ASIO output channels required")?;
            for channel in channels {
                println!(
                    "ASIO selected channel={:?}",
                    control.channel_info(*channel, false)?
                );
            }
            let format = AudioFormat::new(rate as u32, u16::try_from(channels.len())?)?;
            println!(
                "ASIO selected registration={driver:?}; actual format={format:?}; timer/latency bounds are supplied estimates, not native accuracy proof"
            );
            Ok(Self::Asio {
                control,
                window,
                format,
            })
        }
    }
    pub(super) fn format(&self) -> AudioFormat {
        match self {
            Self::Wasapi(request) => request.format().pcm(),
            #[cfg(feature = "asio-sdk")]
            Self::Asio { format, .. } => *format,
        }
    }
    #[cfg_attr(not(feature = "asio-sdk"), allow(unused_variables))]
    pub(super) fn open(self, mixer: Mixer, options: &Options, clock: QpcClock) -> Result<Output> {
        match self {
            Self::Wasapi(request) => Ok(Output::Wasapi(WasapiBackend.open(
                request,
                mixer,
                clock,
                WasapiOptions::default(),
            )?)),
            #[cfg(feature = "asio-sdk")]
            Self::Asio {
                control, window, ..
            } => {
                // Keep the driver local newer than the HWND binding so every
                // setup rejection drops/releases it before destroying sysref.
                let mut control = control;
                let request = match options.buffer {
                    BufferRequest::DeviceDefault => AsioBufferRequest::DriverPreferred,
                    BufferRequest::Frames(n) => AsioBufferRequest::Frames(n),
                    _ => return Err("ASIO buffer must be preferred or exact frames".into()),
                };
                let constraints = control.buffer_constraints()?;
                let resolved = constraints.resolve(request)?;
                if resolved as usize > AudioLimits::MAX_RENDER_FRAMES {
                    return Err("ASIO preferred buffer exceeds core render ceiling".into());
                }
                println!(
                    "ASIO requested={request:?}; reported={constraints:?}; resolved={resolved} frames"
                );
                let stream = AsioStream::prepare_with_clock(
                    control,
                    mixer,
                    options
                        .output_channels
                        .clone()
                        .ok_or("ASIO output channels required")?,
                    request,
                    clock,
                )?;
                Ok(Output::Asio(AsioOutput {
                    stream,
                    _window: window,
                    clock,
                    anchor: None,
                    timer_error: options.asio_timer_error,
                    drift_error: options.asio_drift_error,
                    latency_error: options.asio_latency_error,
                    age: options.asio_anchor_age,
                }))
            }
        }
    }
}
pub(super) enum Output {
    Wasapi(WasapiStream),
    #[cfg(feature = "asio-sdk")]
    Asio(AsioOutput),
}
impl Output {
    pub(super) fn start(&mut self) -> Result<()> {
        match self {
            Self::Wasapi(s) => Ok(s.start()?),
            #[cfg(feature = "asio-sdk")]
            Self::Asio(s) => Ok(s.stream.start()?),
        }
    }
    pub(super) fn stop(&mut self) -> Result<()> {
        match self {
            Self::Wasapi(s) => {
                let result = s.stop();
                println!(
                    "joined WASAPI render-start cadence={:?}; software scheduling, physical jitter unmeasured",
                    s.render_cadence()
                );
                Ok(result?)
            }
            #[cfg(feature = "asio-sdk")]
            Self::Asio(s) => {
                let result = s.stream.stop();
                println!(
                    "closed ASIO render-start QPC cadence={:?}; software scheduling, driver system time and physical jitter distinct",
                    s.stream.render_cadence()
                );
                Ok(result?)
            }
        }
    }
    pub(super) fn description(&mut self) -> String {
        match self {
            Self::Wasapi(s) => format!(
                "WASAPI applied={:?} snapshot={:?}",
                s.configuration(),
                s.snapshot()
            ),
            #[cfg(feature = "asio-sdk")]
            Self::Asio(s) => format!(
                "ASIO software prepared/raw native diagnostics={:?}; prepared is not audible position; raw ns is not QPC",
                s.stream.snapshot()
            ),
        }
    }
    pub(super) fn render_report(&mut self) -> Result<Option<RenderReport>> {
        match self {
            Self::Wasapi(s) => Ok(s.snapshot().render),
            #[cfg(feature = "asio-sdk")]
            Self::Asio(s) => {
                let snapshot = s.checked()?;
                Ok(if snapshot.telemetry_available {
                    snapshot.render
                } else {
                    None
                })
            }
        }
    }
    pub(super) fn schedule(&mut self, rate: u32) -> Result<ClockPoint> {
        match self {
            Self::Wasapi(s) => super::native::schedule_wasapi(s, rate),
            #[cfg(feature = "asio-sdk")]
            Self::Asio(s) => {
                let snapshot = s.checked()?;
                if !snapshot.telemetry_available {
                    return Err("ASIO software prepared frontier unavailable".into());
                }
                let nanos = (u128::from(snapshot.prepared_frames) * 1_000_000_000)
                    .div_ceil(u128::from(rate));
                Ok(ClockPoint {
                    domain: OUTPUT,
                    timestamp: Timestamp::from_nanos(i64::try_from(nanos)?),
                })
            }
        }
    }
    pub(super) fn calibrate(
        &mut self,
        options: &Options,
        extent: i64,
        bgm: &mut BgmSession,
        producer: &mut CommandProducer,
    ) -> Result<(Transport, ClockMappingQuality)> {
        match self {
            Self::Wasapi(s) => {
                let relation = super::native::presentation_wasapi(s, extent, bgm, producer)?;
                Ok((
                    relation.transport(options.song_origin()?)?,
                    relation.quality(),
                ))
            }
            #[cfg(feature = "asio-sdk")]
            Self::Asio(s) => {
                use beatkernel::time::{ClockInterval, ExtrapolationPolicy};
                let deadline = Instant::now() + WallDuration::from_secs(2);
                let mut first: Option<AsioPresentationObservation> = None;
                while Instant::now() < deadline {
                    feed_rendered(bgm, s.checked()?.render, |command| {
                        producer.try_push(command)
                    })?;
                    if let Some(observation) = s.observation()? {
                        if let Some(previous) = first {
                            let midpoint = |value: AsioPresentationObservation| {
                                let before = i128::from(value.host.before.timestamp.as_nanos());
                                before
                                    + (i128::from(value.host.after.timestamp.as_nanos()) - before)
                                        / 2
                            };
                            if midpoint(observation) < midpoint(previous) {
                                return Err("ASIO presentation host midpoint regressed".into());
                            }
                            if midpoint(observation) > midpoint(previous)
                                && observation.render.start_frame > previous.render.start_frame
                                && observation.render.start_frame
                                    >= previous.render.start_frame + previous.render.frames as u64
                            {
                                let relation = AsioPresentationClock::from_observations(
                                    previous,
                                    observation,
                                    ClockInterval {
                                        start: Timestamp::ZERO,
                                        end: Timestamp::from_nanos(extent),
                                    },
                                    ExtrapolationPolicy::Bounded {
                                        before: Duration::from_nanos(extent),
                                        after: Duration::from_nanos(extent),
                                    },
                                    None,
                                )?;
                                return Ok((
                                    relation.transport(options.song_origin()?)?,
                                    relation.quality(),
                                ));
                            }
                        } else {
                            first = Some(observation);
                        }
                    }
                    std::thread::sleep(WallDuration::from_millis(1));
                }
                Err("no progressing coherent ASIO presentation within two seconds".into())
            }
        }
    }
    pub(super) fn observe(
        &mut self,
        discipline: &mut PresentationDiscipline,
    ) -> Result<Option<ObservationAdmission>> {
        match self {
            Self::Wasapi(s) => super::native::observe_wasapi(s, discipline),
            #[cfg(feature = "asio-sdk")]
            Self::Asio(s) => match s.observation()? {
                Some(observation) => Ok(Some(discipline.observe_asio(observation)?)),
                None => Ok(None),
            },
        }
    }
    pub(super) fn seed(
        &mut self,
        discipline: &mut PresentationDiscipline,
        bgm: &mut BgmSession,
        producer: &mut CommandProducer,
    ) -> Result<()> {
        match self {
            Self::Wasapi(s) => super::native::seed_wasapi(s, discipline, bgm, producer),
            #[cfg(feature = "asio-sdk")]
            Self::Asio(_) => {
                let deadline = Instant::now() + WallDuration::from_secs(2);
                while Instant::now() < deadline {
                    let admission = self.observe(discipline)?;
                    feed_rendered(bgm, self.render_report()?, |command| {
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
                Err("no progressing ASIO discipline seed within two seconds".into())
            }
        }
    }
}
#[cfg(feature = "asio-sdk")]
pub(super) struct AsioOutput {
    // Drop order and explicit Drop guarantee close/drain before HWND destruction.
    stream: AsioStream,
    _window: Window,
    clock: QpcClock,
    anchor: Option<MultimediaClockAnchor>,
    timer_error: u64,
    drift_error: u64,
    latency_error: u64,
    age: u64,
}
#[cfg(feature = "asio-sdk")]
impl AsioOutput {
    fn pump_driver_messages(&self) -> Result<()> {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage, WM_CLOSE, WM_QUIT,
        };
        // SAFETY: initialized POD message storage, owned by the native window
        // thread. The filter retains physical-input messages for the common
        // gameplay pump; this bounded pass only services the driver's sysref.
        let mut message: MSG = unsafe { std::mem::zeroed() };
        for _ in 0..64 {
            if unsafe { PeekMessageW(&mut message, self._window.hwnd, 0, 0, PM_REMOVE) } == 0 {
                break;
            }
            if message.message == WM_CLOSE || message.message == WM_QUIT {
                return Err("ASIO driver host window requested stop".into());
            }
            // SAFETY: a real initialized native message on the owning thread;
            // the registered procedure is stateless and does not unwind.
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        Ok(())
    }
    fn checked(
        &mut self,
    ) -> Result<beatkernel_platform::windows::asio::stream::AsioStreamSnapshot> {
        let snapshot = self.stream.snapshot()?;
        if snapshot.phase != AsioStreamPhase::Running
            || snapshot.native.faults.0 != 0
            || snapshot.native.render_error != 0
        {
            return Err(format!(
                "ASIO terminal/native fault (including overload or clock exhaustion): {snapshot:?}"
            )
            .into());
        }
        Ok(snapshot)
    }
    fn observation(&mut self) -> Result<Option<AsioPresentationObservation>> {
        self.pump_driver_messages()?;
        self.checked()?;
        let now = self.clock.sample()?.normalized;
        let refresh = self.anchor.as_ref().is_none_or(|anchor| {
            i128::from(now.timestamp.as_nanos()) - i128::from(anchor.after().timestamp.as_nanos())
                >= i128::from(self.age) / 2
        });
        if refresh {
            let receipt = self.clock.sample_multimedia()?;
            self.anchor = Some(MultimediaClockAnchor::new(
                receipt.milliseconds,
                receipt.before.normalized,
                receipt.after.normalized,
                self.age,
                self.timer_error,
                self.drift_error,
            )?);
        }
        match self.stream.presentation_observation(
            self.anchor.as_ref().unwrap(),
            &self.clock,
            self.latency_error,
            ClockPoint {
                domain: OUTPUT,
                timestamp: Timestamp::ZERO,
            },
        ) {
            Ok(observation) => Ok(Some(observation)),
            Err(
                AsioPresentationError::Unavailable
                | AsioPresentationError::Clock(
                    beatkernel_platform::audio::asio::MultimediaClockError::Expired,
                ),
            ) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}
#[cfg(feature = "asio-sdk")]
impl Drop for AsioOutput {
    fn drop(&mut self) {
        let _ = self.stream.stop();
    }
}
