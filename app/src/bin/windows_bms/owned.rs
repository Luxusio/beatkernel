//! Native composition around the shared output owner; ASIO keeps its HWND owner.
use super::live_output::{Output, StartupEvidence};
use super::*;
use beatkernel::audio::{
    ChannelMatrix, Mixer, OutputFrameBasis, OutputOpenFailure, RenderReport, StoppedMixerSource,
};
use beatkernel::time::{ClockPair, ClockPoint};
use beatkernel_bms_runtime::{
    gameplay::output::{
        adapters::{player::PlayerOutputUi, remix::RemixedOutputBackend},
        application::{owner::GameplayOutputOwner, requests::GameplayOutputUi},
        ports::{OutputChannelRemixBackend, OutputReplacementBackend},
    },
    gameplay_presentation::{GameplayAudioOutputContext, GameplayOutputContext},
    live_pause::LivePauseObservation,
    native_end::{EndBoundary, NativeEnd},
};
use beatkernel_platform::{
    audio::{
        presentation::discipline::PresentationDiscipline, AudioOutputStream, AudioStreamRequest,
    },
    windows::{
        audio::{WasapiBackend, WasapiOptions},
        clock::QpcClock,
    },
};

#[derive(Debug)]
pub(super) struct NativeOutputError(Box<dyn Error>);
impl std::fmt::Display for NativeOutputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl Error for NativeOutputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.0.as_ref())
    }
}
pub(super) struct OwnedOutput {
    pub(super) native: Output,
    epoch: u64,
    matrix: Option<ChannelMatrix>,
    report: Option<RenderReport>,
    accepted: Option<StartupEvidence>,
    source_channels: u16,
    #[cfg_attr(not(feature = "asio-sdk"), allow(dead_code))]
    clock: QpcClock,
}
impl OwnedOutput {
    fn new(
        native: Output,
        epoch: u64,
        matrix: Option<ChannelMatrix>,
        source_channels: u16,
        clock: QpcClock,
    ) -> Self {
        Self {
            native,
            epoch,
            matrix,
            report: None,
            accepted: None,
            source_channels,
            clock,
        }
    }
    fn basis(&self) -> OutputFrameBasis {
        match &self.native {
            Output::Wasapi(stream) => stream.frame_basis(),
            #[cfg(feature = "asio-sdk")]
            Output::Asio(output) => output.stream.frame_basis(),
        }
    }
}
impl StoppedMixerSource for OwnedOutput {
    type Error = NativeOutputError;
    fn take_stopped_mixer(&mut self) -> std::result::Result<Option<Mixer>, Self::Error> {
        match &mut self.native {
            Output::Wasapi(stream) => stream
                .take_stopped_mixer()
                .map_err(|e| NativeOutputError(e.into())),
            #[cfg(feature = "asio-sdk")]
            Output::Asio(output) => output
                .stream
                .take_stopped_mixer()
                .map_err(|e| NativeOutputError(e.into())),
        }
    }
}
pub(super) struct WindowsReplacementBackend {
    clock: QpcClock,
}
pub(super) enum WindowsRequest {
    Wasapi(AudioStreamRequest),
    #[cfg(feature = "asio-sdk")]
    Asio(super::live_output::AsioLiveConfig),
}
impl WindowsReplacementBackend {
    fn open_native(
        &mut self,
        request: WindowsRequest,
        mixer: Mixer,
        epoch: u64,
        matrix: Option<ChannelMatrix>,
    ) -> std::result::Result<OwnedOutput, OutputOpenFailure<NativeOutputError, OwnedOutput>> {
        let source_channels = mixer.configuration().format().channels();
        let converter_matrix = match matrix
            .as_ref()
            .map(|m| ChannelMatrix::new(m.source_channels(), m.target_channels(), m.coefficients()))
            .transpose()
        {
            Ok(matrix) => matrix,
            Err(error) => {
                return Err(OutputOpenFailure::recovered(
                    NativeOutputError(error.into()),
                    Some(mixer),
                ));
            }
        };
        #[cfg(feature = "asio-sdk")]
        if let WindowsRequest::Asio(config) = request {
            return match super::live_output::AsioOutput::reopen(
                config,
                mixer,
                self.clock,
                converter_matrix,
            ) {
                Ok(output) => Ok(OwnedOutput::new(
                    Output::Asio(output),
                    epoch,
                    matrix,
                    source_channels,
                    self.clock,
                )),
                Err(failure) => {
                    let (error, mixer, pending, cleanup) = failure.into_parts();
                    let failure = match pending {
                        Some(output) => OutputOpenFailure::pending(
                            NativeOutputError(error),
                            OwnedOutput::new(
                                Output::Asio(output),
                                epoch,
                                matrix,
                                source_channels,
                                self.clock,
                            ),
                        ),
                        None => OutputOpenFailure::recovered(NativeOutputError(error), mixer),
                    };
                    Err(match cleanup {
                        Some(error) => failure.with_cleanup_error(NativeOutputError(error)),
                        None => failure,
                    })
                }
            };
        }
        let request = match request {
            WindowsRequest::Wasapi(request) => request,
            #[cfg(feature = "asio-sdk")]
            WindowsRequest::Asio(_) => unreachable!("ASIO request handled above"),
        };
        let result = match converter_matrix {
            Some(matrix) => WasapiBackend.open_remixed_recoverable(
                request,
                mixer,
                self.clock,
                WasapiOptions::default(),
                matrix,
            ),
            None => {
                WasapiBackend.open_recoverable(request, mixer, self.clock, WasapiOptions::default())
            }
        };
        result
            .map(|stream| {
                OwnedOutput::new(
                    Output::Wasapi(stream),
                    epoch,
                    matrix,
                    source_channels,
                    self.clock,
                )
            })
            .map_err(|failure| {
                let (error, mixer) = failure.into_parts();
                OutputOpenFailure::recovered(NativeOutputError(error.into()), mixer)
            })
    }
}
impl OutputReplacementBackend for WindowsReplacementBackend {
    type Presentation = PresentationDiscipline;
    type Output = OwnedOutput;
    type Request = WindowsRequest;
    type Error = NativeOutputError;
    fn open(
        &mut self,
        request: Self::Request,
        mixer: Mixer,
        epoch: u64,
    ) -> std::result::Result<OwnedOutput, OutputOpenFailure<Self::Error, OwnedOutput>> {
        self.open_native(request, mixer, epoch, None)
    }
    fn retire(&mut self, output: &mut OwnedOutput) -> std::result::Result<(), Self::Error> {
        output.native.stop().map_err(NativeOutputError)
    }
    fn start(&mut self, output: &mut OwnedOutput) -> std::result::Result<(), Self::Error> {
        output.native.start().map_err(NativeOutputError)
    }
    fn epoch(&self, output: &OwnedOutput) -> u64 {
        output.epoch
    }
    fn basis(&self, output: &OwnedOutput) -> OutputFrameBasis {
        output.basis()
    }
    fn observe(
        &mut self,
        output: &mut OwnedOutput,
        presentation: &mut PresentationDiscipline,
    ) -> std::result::Result<(), Self::Error> {
        output.accepted = None;
        if let Some(observation) = output
            .native
            .startup_observation(presentation)
            .map_err(NativeOutputError)?
        {
            output.accepted = Some(observation.evidence);
        }
        output.report = output.native.render_report().map_err(NativeOutputError)?;
        Ok(())
    }
    fn render_report(
        &self,
        output: &OwnedOutput,
    ) -> std::result::Result<Option<RenderReport>, Self::Error> {
        Ok(output.report)
    }
    fn pause_observation(
        &self,
        output: &OwnedOutput,
        pair: ClockPair,
        _now: ClockPoint,
    ) -> std::result::Result<LivePauseObservation, Self::Error> {
        match &output.native {
            Output::Wasapi(_) => Ok(LivePauseObservation::Point(pair)),
            #[cfg(feature = "asio-sdk")]
            Output::Asio(_) => Ok(super::asio_pause_observation(
                match output.accepted {
                    Some(StartupEvidence::Asio(value)) => Some(value),
                    _ => None,
                },
                _now,
            )),
        }
    }
    fn observe_end(
        &self,
        output: &OwnedOutput,
        end: &mut NativeEnd,
        pair: ClockPair,
        report: Option<RenderReport>,
    ) -> beatkernel_bms_runtime::native_gameplay::NativeGameplayResult<Option<EndBoundary>> {
        match &output.native {
            Output::Wasapi(_) => Ok(end.observe(report, pair)?),
            #[cfg(feature = "asio-sdk")]
            Output::Asio(_) => match output.accepted {
                Some(StartupEvidence::Asio(value)) => Ok(end.observe_asio(value)?),
                _ => Ok(None),
            },
        }
    }
}
impl beatkernel_bms_runtime::gameplay::output::ports::OriginalNativeOutputBackend
    for WindowsReplacementBackend
{
    fn observe_native(
        &mut self,
        output: &mut OwnedOutput,
    ) -> std::result::Result<
        Option<beatkernel_bms_runtime::native_audio_presentation::NativeAudioSnapshot>,
        Self::Error,
    > {
        let snapshot = output
            .native
            .native_observation(output.epoch)
            .map_err(NativeOutputError)?;
        // This is raw native IO telemetry, not accepted ASIO pause/end evidence.
        // Both reads complete before the shared application owner admits a pair.
        let report = output
            .native
            .native_render_report()
            .map_err(NativeOutputError)?;
        output.report = report;
        Ok(snapshot)
    }
}
impl OutputChannelRemixBackend for WindowsReplacementBackend {
    fn open_remixed(
        &mut self,
        request: Self::Request,
        mixer: Mixer,
        epoch: u64,
        matrix: ChannelMatrix,
    ) -> std::result::Result<OwnedOutput, OutputOpenFailure<Self::Error, OwnedOutput>> {
        self.open_native(request, mixer, epoch, Some(matrix))
    }
}
pub(super) type WindowsOutputOwner =
    GameplayOutputOwner<RemixedOutputBackend<WindowsReplacementBackend>>;
pub(super) fn owner(native: Output, clock: QpcClock) -> WindowsOutputOwner {
    let source_channels = match &native {
        Output::Wasapi(stream) => stream.configuration().format.channels(),
        #[cfg(feature = "asio-sdk")]
        Output::Asio(output) => output.live.channels.len() as u16,
    };
    WindowsOutputOwner::new(
        RemixedOutputBackend::new(WindowsReplacementBackend { clock }),
        OwnedOutput::new(native, 0, None, source_channels, clock),
    )
}
pub(super) fn stream(owner: &mut WindowsOutputOwner) -> Result<&mut Output> {
    Ok(&mut owner
        .current_mut()
        .ok_or("current Windows output unavailable")?
        .native)
}
pub(super) fn basis(owner: &WindowsOutputOwner) -> Result<OutputFrameBasis> {
    Ok(owner
        .current()
        .ok_or("current Windows output unavailable")?
        .basis())
}
fn capability(
    output: &OwnedOutput,
) -> std::result::Result<
    beatkernel_bms_runtime::gameplay::output::domain::control::OutputCapability,
    String,
> {
    match &output.native {
        Output::Wasapi(stream) => super::output_settings::switch_capability(
            super::output_settings::capability(stream.configuration(), output.matrix.as_ref())?,
            Backend::Wasapi,
            None,
        ),
        #[cfg(feature = "asio-sdk")]
        Output::Asio(native) => {
            native.applied_buffer_frames().map_err(|e| e.to_string())?;
            let cap = super::output_settings::asio_capability(
                &native.live.registration.id.clsid,
                native.live.buffer,
                &native.live.channels,
                output.matrix.as_ref(),
            )?;
            let cap =
                super::output_settings::asio_clock_capability(cap, native.live.clock_bounds())?;
            let view = match native.live.registration.id.view {
                beatkernel_platform::windows::asio::AsioRegistryView::Native => AsioView::Native,
                beatkernel_platform::windows::asio::AsioRegistryView::Bits32 => AsioView::Bits32,
                beatkernel_platform::windows::asio::AsioRegistryView::Bits64 => AsioView::Bits64,
            };
            super::output_settings::switch_capability(cap, Backend::Asio, Some(view))
        }
    }
}
pub(super) struct WindowsOutputUi {
    bridge: GameplayOutputUi<PlayerOutputUi>,
}
impl WindowsOutputUi {
    pub(super) fn new(owner: &WindowsOutputOwner, enabled: bool) -> Result<Self> {
        let mut bridge = GameplayOutputUi::new(PlayerOutputUi);
        let cap = if enabled {
            match owner.current() {
                Some(output) => Some(capability(output)?),
                _ => None,
            }
        } else {
            None
        };
        bridge.advertise(cap)?;
        Ok(Self { bridge })
    }
    pub(super) fn pending(&self) -> bool {
        self.bridge.pending()
    }
    fn map_request(
        request: &beatkernel_bms_runtime::gameplay::output::domain::control::OutputRequest,
        output: &OwnedOutput,
    ) -> std::result::Result<
        beatkernel_bms_runtime::gameplay::output::domain::remix::RemixedOutputRequest<
            WindowsRequest,
        >,
        String,
    > {
        let current = capability(output)?;
        let wasapi = match &output.native {
            Output::Wasapi(stream) => Some(stream.configuration().requested.clone()),
            #[cfg(feature = "asio-sdk")]
            Output::Asio(_) => None,
        };
        let source = beatkernel::audio::AudioFormat::new(
            output.basis().sample_rate(),
            output.source_channels,
        )
        .map_err(|e| e.to_string())?;
        let target = super::output_settings::plan_target(
            &current,
            wasapi.as_ref(),
            source,
            output.matrix.as_ref(),
            &request.args,
            cfg!(feature = "asio-sdk"),
        )?;
        match target {
            super::output_settings::TargetPlan::Wasapi(plan) => {
                let default = if plan.device.is_none() {
                    use beatkernel_platform::audio::{AudioDeviceState, AudioOutputBackend};
                    Some(
                        WasapiBackend
                            .devices()
                            .map_err(|e| e.to_string())?
                            .into_iter()
                            .find(|d| d.state == AudioDeviceState::Active && d.default_multimedia)
                            .ok_or("no active OS default multimedia output")?
                            .id,
                    )
                } else {
                    None
                };
                let request = plan.into_request(default)?;
                Ok(
                    beatkernel_bms_runtime::gameplay::output::domain::remix::RemixedOutputRequest {
                        native: WindowsRequest::Wasapi(request.native),
                        matrix: request.matrix,
                    },
                )
            }
            super::output_settings::TargetPlan::Asio(plan) => {
                #[cfg(not(feature = "asio-sdk"))]
                {
                    let _ = plan;
                    Err("ASIO requires build feature asio-sdk".into())
                }
                #[cfg(feature = "asio-sdk")]
                {
                    use beatkernel_platform::windows::asio::{
                        enumerate_asio_drivers, AsioEnumerationLimits, AsioRegistryView,
                    };
                    let view = match plan.view {
                        AsioView::Native => AsioRegistryView::Native,
                        AsioView::Bits32 => AsioRegistryView::Bits32,
                        AsioView::Bits64 => AsioRegistryView::Bits64,
                    };
                    let drivers = enumerate_asio_drivers(view, AsioEnumerationLimits::default())
                        .map_err(|e| e.to_string())?;
                    let index = super::output_settings::asio_driver_index(
                        &plan.device,
                        drivers.iter().map(|d| d.id.clsid.as_str()),
                    )?;
                    let receipt = output
                        .clock
                        .sample_multimedia()
                        .map_err(|e| e.to_string())?;
                    beatkernel_platform::audio::asio::MultimediaClockAnchor::new(
                        receipt.milliseconds,
                        receipt.before.normalized,
                        receipt.after.normalized,
                        plan.bounds.age,
                        plan.bounds.timer,
                        plan.bounds.drift,
                    )
                    .map_err(|e| e.to_string())?;
                    let matrix = plan.matrix.clone();
                    let config = super::live_output::AsioLiveConfig::from_target(
                        drivers[index].clone(),
                        plan,
                    );
                    Ok(beatkernel_bms_runtime::gameplay::output::domain::remix::RemixedOutputRequest { native:WindowsRequest::Asio(config),matrix })
                }
            }
        }
    }
    pub(super) fn service(
        &mut self,
        owner: &mut WindowsOutputOwner,
        context: GameplayOutputContext<'_, PresentationDiscipline>,
        now: ClockPoint,
    ) -> Result<bool> {
        self.bridge
            .service(owner, context, now, &mut Self::map_request, &mut capability)
    }
    pub(super) fn service_audio(
        &mut self,
        owner: &mut WindowsOutputOwner,
        context: GameplayAudioOutputContext<'_>,
        now: ClockPoint,
    ) -> Result<bool> {
        self.bridge
            .service_audio(owner, context, now, &mut Self::map_request, &mut capability)
    }
}
