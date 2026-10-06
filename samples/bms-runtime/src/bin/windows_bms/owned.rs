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
        ports::{OutputReplacementBackend, OutputChannelRemixBackend},
    },
    gameplay_presentation::GameplayOutputContext,
    live_pause::LivePauseObservation,
    native_end::{EndBoundary, NativeEnd},
};
use beatkernel_platform::{
    audio::{
        AudioOutputStream, AudioStreamRequest, presentation::discipline::PresentationDiscipline,
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
}
impl OwnedOutput {
    fn new(native: Output, epoch: u64, matrix: Option<ChannelMatrix>) -> Self {
        Self {
            native,
            epoch,
            matrix,
            report: None,
            accepted: None,
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
                Ok(output) => Ok(OwnedOutput::new(Output::Asio(output), epoch, matrix)),
                Err(failure) => {
                    let (error, mixer, pending, cleanup) = failure.into_parts();
                    let failure = match pending {
                        Some(output) => OutputOpenFailure::pending(
                            NativeOutputError(error),
                            OwnedOutput::new(Output::Asio(output), epoch, matrix),
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
            .map(|stream| OwnedOutput::new(Output::Wasapi(stream), epoch, matrix))
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
    WindowsOutputOwner::new(
        RemixedOutputBackend::new(WindowsReplacementBackend { clock }),
        OwnedOutput::new(native, 0, None),
    )
}
pub(super) fn stream(owner: &mut WindowsOutputOwner) -> Result<&mut Output> {
    Ok(&mut owner
        .current_mut()
        .ok_or("current Windows output unavailable")?
        .native)
}
fn capability(
    output: &OwnedOutput,
) -> std::result::Result<
    beatkernel_bms_runtime::gameplay::output::domain::control::OutputCapability,
    String,
> {
    match &output.native {
        Output::Wasapi(stream) => {
            super::output_settings::capability(stream.configuration(), output.matrix.as_ref())
        }
        #[cfg(feature = "asio-sdk")]
        Output::Asio(native) => {
            native.applied_buffer_frames().map_err(|e| e.to_string())?;
            super::output_settings::asio_capability(
                native.live.buffer,
                &native.live.channels,
                output.matrix.as_ref(),
            )
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
    pub(super) fn service(
        &mut self,
        owner: &mut WindowsOutputOwner,
        context: GameplayOutputContext<'_, PresentationDiscipline>,
        now: ClockPoint,
    ) -> Result<bool> {
        self.bridge.service(
            owner,
            context,
            now,
            &mut |request, output| match &output.native {
                Output::Wasapi(stream) => super::output_settings::request(
                    stream.configuration().requested.clone(),
                    output.matrix.as_ref(),
                    &request.args,
                ).map(|request| beatkernel_bms_runtime::gameplay::output::domain::remix::RemixedOutputRequest {
                    native: WindowsRequest::Wasapi(request.native), matrix: request.matrix,
                }),
                #[cfg(feature = "asio-sdk")]
                Output::Asio(native) => {
                    let (buffer, channels, matrix) = super::output_settings::asio_request(
                        &native.live.registration.id.clsid, native.live.buffer,
                        &native.live.channels,
                        output.matrix.as_ref(), &request.args)?;
                    let mut config = native.live.clone();
                    config.buffer = buffer;
                    config.channels = channels;
                    Ok(beatkernel_bms_runtime::gameplay::output::domain::remix::RemixedOutputRequest {
                        native: WindowsRequest::Asio(config), matrix,
                    })
                },
            },
            &mut capability,
        )
    }
}
