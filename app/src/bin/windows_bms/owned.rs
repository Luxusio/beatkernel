//! Native composition around the shared output owner; ASIO keeps its HWND owner.
use super::live_output::{Output, StartupEvidence, WindowsSourceEvidence};
use super::*;
use beatkernel::audio::{
    AudioFormat, ChannelMatrix, Mixer, OutputFrameBasis, OutputOpenFailure, RenderReport,
    StoppedMixerSource,
};
use beatkernel::time::{ClockPair, ClockPoint};
use beatkernel_bms_runtime::{
    gameplay::output::{
        adapters::{
            player::PlayerOutputUi,
            remix::RemixedOutputBackend,
            wasapi::{WasapiCompleteReplacementBackend, WasapiCompleteReplacementOutput},
        },
        application::{owner::GameplayOutputOwner, requests::GameplayOutputUi},
        ports::{OutputChannelRemixBackend, OutputReplacementBackend},
    },
    gameplay_presentation::{GameplayAudioOutputContext, GameplayOutputContext},
    live_pause::LivePauseObservation,
    native_audio_presentation::NativeAudioPresentation,
    native_end::{EndBoundary, NativeEnd},
    native_playback_phase::NativePlaybackMapping,
};
use beatkernel_platform::{
    audio::{
        presentation::discipline::PresentationDiscipline, AudioOutputStream, AudioStreamRequest,
        NativeOutputState,
    },
    windows::{
        audio::{SourceRetirementTicket, WasapiBackend, WasapiOptions},
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
impl beatkernel_bms_runtime::gameplay::output::ports::OriginalCoordinateNativeOutputBackend
    for WindowsReplacementBackend
{
    fn observe_native_coordinate(
        &mut self,
        output: &mut OwnedOutput,
    ) -> std::result::Result<
        Option<beatkernel_bms_runtime::native_output_coordinate::CoordinateNativeAudioSnapshot>,
        Self::Error,
    > {
        let observation = output
            .native
            .native_coordinate_observation(output.epoch)
            .map_err(NativeOutputError)?;
        // Keep the report from this original observation, never a later IO read.
        output.report = observation.map(|value| {
            use beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence;
            match value.snapshot().evidence {
                OriginalNativePresentationEvidence::Wasapi { snapshot, .. } => snapshot.render,
                #[cfg(feature = "asio-sdk")]
                OriginalNativePresentationEvidence::Asio { observation, .. } => Some(observation.render),
                _ => None,
            }
        }).flatten();
        Ok(observation)
    }
}
impl beatkernel_bms_runtime::gameplay::output::ports::SourcePlaybackNativeOutputBackend
    for WindowsReplacementBackend
{
    type NativeEvidence = WindowsSourceEvidence;
    fn source_playback_at(
        &self,
        output: &OwnedOutput,
        query: &beatkernel::audio::InterpolatedTargetTime,
        identity: beatkernel_bms_runtime::gameplay::output::ports::SourcePlaybackIdentity,
    ) -> std::result::Result<
        Option<
            beatkernel_bms_runtime::gameplay::output::ports::NativeSourcePlaybackQuery<
                Self::NativeEvidence,
            >,
        >,
        Self::Error,
    > {
        output
            .native
            .source_playback_at(output.epoch, query, identity)
            .map_err(NativeOutputError)
    }
    fn observe_native_playback(
        &mut self,
        output: &mut OwnedOutput,
        identity: beatkernel_bms_runtime::gameplay::output::ports::SourcePlaybackIdentity,
    ) -> std::result::Result<
        Option<
            beatkernel_bms_runtime::gameplay::output::ports::NativePlaybackObservation<
                Self::NativeEvidence,
            >,
        >,
        Self::Error,
    > {
        let observation = output
            .native
            .native_playback_observation(output.epoch, identity)
            .map_err(NativeOutputError)?;
        // A Source query remains immutable; only a coherent compound observation
        // updates this raw report cache. It grants no accepted pause/end evidence.
        output.report = observation
            .as_ref()
            .and_then(|value| value.source().facts().render);
        Ok(observation)
    }
}
impl beatkernel_bms_runtime::gameplay::output::ports::SourceHistoryRetirementNativeOutputBackend
    for WindowsReplacementBackend
{
    fn request_source_history_retirement(
        &mut self,
        output: &mut OwnedOutput,
        expected_epoch: u64,
        expected_basis: OutputFrameBasis,
        identity: beatkernel_bms_runtime::gameplay::output::ports::SourcePlaybackIdentity,
        source_pin: u64,
    ) -> std::result::Result<Option<u64>, Self::Error> {
        use beatkernel_bms_runtime::gameplay::output::ports::SourcePlaybackPacketError;
        if output.epoch != expected_epoch {
            return Err(NativeOutputError(
                SourcePlaybackPacketError::EpochMismatch.into(),
            ));
        }
        if output.basis() != expected_basis {
            return Err(NativeOutputError(
                SourcePlaybackPacketError::BasisMismatch.into(),
            ));
        }
        output
            .native
            .request_source_history_retirement(expected_basis, identity, source_pin)
            .map_err(NativeOutputError)
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
type LegacyOutputOwner = GameplayOutputOwner<RemixedOutputBackend<WindowsReplacementBackend>>;
type CompleteOutputOwner = GameplayOutputOwner<
    RemixedOutputBackend<WasapiCompleteReplacementBackend>,
    NativeOutputState,
    OutputFrameBasis,
    SourceRetirementTicket,
>;
pub(super) enum WindowsOutputOwner {
    Legacy(LegacyOutputOwner),
    Complete {
        owner: CompleteOutputOwner,
        source: AudioFormat,
    },
}
pub(super) fn owner(native: Output, clock: QpcClock) -> WindowsOutputOwner {
    let source_channels = match &native {
        Output::Wasapi(stream) => stream.configuration().format.channels(),
        #[cfg(feature = "asio-sdk")]
        Output::Asio(output) => output.live.channels.len() as u16,
    };
    WindowsOutputOwner::Legacy(LegacyOutputOwner::new(
        RemixedOutputBackend::new(WindowsReplacementBackend { clock }),
        OwnedOutput::new(native, 0, None, source_channels, clock),
    ))
}
pub(super) fn complete_owner(
    request: AudioStreamRequest,
    mixer: Mixer,
    clock: QpcClock,
) -> Result<WindowsOutputOwner> {
    // This identity belongs to the original Mixer, even after a channel remix.
    let source = mixer.configuration().format();
    let backend = RemixedOutputBackend::new(WasapiCompleteReplacementBackend::new(
        clock,
        WasapiOptions::default(),
        beatkernel_bms_runtime::native_audio_startup::SOURCE_COORDINATE_HISTORY_HEADROOM,
    ));
    let owner = CompleteOutputOwner::open_initial_with_ticket(
        backend,
        beatkernel_bms_runtime::gameplay::output::domain::remix::RemixedOutputRequest {
            native: request,
            matrix: None,
        },
        NativeOutputState::from_mixer(mixer),
        0,
    )?;
    Ok(WindowsOutputOwner::Complete { owner, source })
}
pub(super) fn stream(owner: &mut WindowsOutputOwner) -> Result<&mut Output> {
    let WindowsOutputOwner::Legacy(owner) = owner else {
        return Err("complete Source output cannot be borrowed as a legacy stream".into());
    };
    Ok(&mut owner
        .current_mut()
        .ok_or("current Windows output unavailable")?
        .native)
}
pub(super) fn basis(owner: &WindowsOutputOwner) -> Result<OutputFrameBasis> {
    match owner {
        WindowsOutputOwner::Legacy(owner) => Ok(owner
            .current()
            .ok_or("current Windows output unavailable")?
            .basis()),
        WindowsOutputOwner::Complete { owner, .. } => Ok(owner
            .current()
            .ok_or("current Windows complete output unavailable")?
            .stream()
            .frame_basis()),
    }
}

impl WindowsOutputOwner {
    pub(super) fn last_issued_epoch(&self) -> u64 {
        match self {
            Self::Legacy(o) => o.last_issued_epoch(),
            Self::Complete { owner, .. } => owner.last_issued_epoch(),
        }
    }
    pub(super) fn has_work(&self) -> bool {
        match self {
            Self::Legacy(o) => o.has_work(),
            Self::Complete { owner, .. } => owner.has_work(),
        }
    }
    pub(super) fn replacement_pending(&self) -> bool {
        match self {
            Self::Legacy(o) => o.replacement_pending(),
            Self::Complete { owner, .. } => owner.replacement_pending(),
        }
    }
    pub(super) fn output_clock_suspended(&self) -> bool {
        match self {
            Self::Legacy(o) => o.output_clock_suspended(),
            Self::Complete { owner, .. } => owner.output_clock_suspended(),
        }
    }
    pub(super) fn render_report(&self) -> Option<RenderReport> {
        match self {
            Self::Legacy(o) => o.render_report(),
            Self::Complete { owner, .. } => owner.render_report(),
        }
    }
    pub(super) fn stop(&mut self) -> Result<()> {
        match self {
            Self::Legacy(o) => Ok(o.stop()?),
            Self::Complete { owner, .. } => Ok(owner.stop()?),
        }
    }
    pub(super) fn start(&mut self) -> Result<()> {
        match self {
            Self::Legacy(o) => o
                .current_mut()
                .ok_or("current Windows output unavailable")?
                .native
                .start(),
            Self::Complete { owner, .. } => Ok(owner
                .current_mut()
                .ok_or("current Windows complete output unavailable")?
                .stream_mut()
                .start()?),
        }
    }
    pub(super) fn description(&mut self) -> Option<String> {
        match self {
            Self::Legacy(o) => o.current_mut().map(|output| output.native.description()),
            Self::Complete { owner, .. } => owner.current().map(|output| {
                format!(
                    "WASAPI retained applied={:?} snapshot={:?}",
                    output.stream().configuration(),
                    output.stream().snapshot()
                )
            }),
        }
    }
    pub(super) fn startup_buffer_frames(&mut self) -> Result<u32> {
        match self {
            Self::Legacy(o) => o
                .current_mut()
                .ok_or("current Windows output unavailable")?
                .native
                .startup_buffer_frames(),
            Self::Complete { owner, .. } => Ok(owner
                .current()
                .ok_or("current Windows complete output unavailable")?
                .stream()
                .configuration()
                .buffer_frames),
        }
    }
    pub(super) fn audio_startup_bound(&mut self, rate: u32) -> Result<beatkernel::time::Duration> {
        if let Self::Legacy(o) = self {
            return o
                .current_mut()
                .ok_or("current Windows output unavailable")?
                .native
                .audio_startup_bound(rate);
        }
        if rate == 0 {
            return Err("native output has zero sample rate".into());
        }
        let period = (i128::from(self.startup_buffer_frames()?) * 1_000_000_000 + i128::from(rate)
            - 1)
            / i128::from(rate);
        let bound = period
            .checked_mul(4)
            .and_then(|value| value.checked_add(2_000_000_000))
            .ok_or("native startup bound overflow")?;
        Ok(beatkernel::time::Duration::from_nanos(i64::try_from(
            bound,
        )?))
    }
    pub(super) fn source_coordinate_basis(
        &mut self,
    ) -> Result<Option<beatkernel_bms_runtime::audio_authority::AudioAuthorityCoordinateBasis>>
    {
        match self {
            Self::Legacy(o) => o
                .current_mut()
                .ok_or("current Windows output unavailable")?
                .native
                .source_coordinate_basis(),
            Self::Complete { owner, .. } => {
                let stream = owner
                    .current()
                    .ok_or("current Windows complete output unavailable")?
                    .stream();
                Ok(Some(beatkernel_bms_runtime::audio_authority::AudioAuthorityCoordinateBasis::SourceCounter {
                    basis: stream.frame_basis(), frequency: stream.clock_frequency(),
                }))
            }
        }
    }
    pub(super) fn startup_render_report(&mut self) -> Result<Option<RenderReport>> {
        match self {
            Self::Legacy(o) => o
                .current_mut()
                .ok_or("current Windows output unavailable")?
                .native
                .render_report(),
            Self::Complete { owner, .. } => Ok(owner.render_report()),
        }
    }
    pub(super) fn observe_native_control(
        &mut self,
        presentation: &mut NativeAudioPresentation,
        identity: beatkernel_bms_runtime::gameplay::output::ports::SourcePlaybackIdentity,
    ) -> Result<Option<beatkernel_bms_runtime::native_coordinate_control::CoordinateControlFacts>>
    {
        match self {
            Self::Legacy(o) => Ok(o.observe_native_control(presentation, identity)?),
            Self::Complete { owner, .. } => {
                owner.observe_conditional_source_native_control(presentation, identity)
            }
        }
    }
    pub(super) fn playback_mapping_at(
        &self,
        presentation: &NativeAudioPresentation,
        query: &beatkernel::audio::InterpolatedTargetTime,
        identity: beatkernel_bms_runtime::gameplay::output::ports::SourcePlaybackIdentity,
    ) -> Result<Option<NativePlaybackMapping>> {
        match self {
            Self::Legacy(o) => Ok(o
                .source_playback_at(presentation, query, identity)?
                .as_ref()
                .map(NativePlaybackMapping::from_source)),
            Self::Complete { .. } => {
                Err("complete Source query requires the mutable conditional device port".into())
            }
        }
    }
    pub(super) fn conditional_playback_mapping_at(
        &mut self,
        presentation: &NativeAudioPresentation,
        query: &beatkernel::audio::InterpolatedTargetTime,
        identity: beatkernel_bms_runtime::gameplay::output::ports::SourcePlaybackIdentity,
    ) -> Result<Option<NativePlaybackMapping>> {
        match self {
            Self::Legacy(o) => Ok(o
                .source_playback_at(presentation, query, identity)?
                .as_ref()
                .map(NativePlaybackMapping::from_source)),
            Self::Complete { owner, .. } => Ok(owner
                .conditional_source_playback_at(presentation, query, identity)?
                .as_ref()
                .map(NativePlaybackMapping::from_source)),
        }
    }
    pub(super) fn request_source_history_retirement_with_source_pin(
        &mut self,
        presentation: &NativeAudioPresentation,
        query: &beatkernel::audio::InterpolatedTargetTime,
        identity: beatkernel_bms_runtime::gameplay::output::ports::SourcePlaybackIdentity,
        source_minimum: Option<u64>,
    ) -> Result<Option<u64>> {
        match self {
            Self::Legacy(o) => Ok(o.request_source_history_retirement_with_source_pin(
                presentation,
                query,
                identity,
                source_minimum,
            )?),
            Self::Complete { owner, .. } => owner
                .request_conditional_source_history_retirement_with_source_pin(
                    presentation,
                    query,
                    identity,
                    source_minimum,
                ),
        }
    }
    pub(super) fn observe_native(
        &mut self,
        presentation: &mut NativeAudioPresentation,
    ) -> Result<()> {
        match self {
            Self::Legacy(o) => Ok(o.observe_native(presentation)?),
            Self::Complete { .. } => {
                Err("complete Source observation requires coordinate identity".into())
            }
        }
    }
    pub(super) fn audio_pause_observation(
        &mut self,
        presentation: &NativeAudioPresentation,
        now: ClockPoint,
    ) -> Result<LivePauseObservation> {
        match self {
            Self::Legacy(o) => Ok(o.audio_pause_observation(presentation, now)?),
            Self::Complete { .. } => {
                Err("complete Source pause requires coordinate control facts".into())
            }
        }
    }
    pub(super) fn observe_audio_end(
        &mut self,
        end: &mut NativeEnd,
        presentation: &NativeAudioPresentation,
    ) -> Result<Option<EndBoundary>> {
        match self {
            Self::Legacy(o) => o.observe_audio_end(end, presentation),
            Self::Complete { .. } => {
                Err("complete Source end uses retained practice end intent".into())
            }
        }
    }
    pub(super) fn observe(&mut self, discipline: &mut PresentationDiscipline) -> Result<()> {
        match self {
            Self::Legacy(o) => Ok(o.observe(discipline)?),
            Self::Complete { .. } => Err("complete Source cannot use scalar presentation".into()),
        }
    }
    pub(super) fn pause_observation(
        &mut self,
        pair: ClockPair,
        now: ClockPoint,
    ) -> Result<LivePauseObservation> {
        match self {
            Self::Legacy(o) => Ok(o.pause_observation(pair, now)?),
            Self::Complete { .. } => Err("complete Source cannot use scalar pause evidence".into()),
        }
    }
    pub(super) fn seed_resume(&mut self, discipline: &mut PresentationDiscipline) -> Result<()> {
        match self {
            Self::Legacy(o) => Ok(o.seed_resume(discipline)?),
            Self::Complete { .. } => {
                Err("complete Source resume requires coordinate adoption".into())
            }
        }
    }
    pub(super) fn observe_end(
        &mut self,
        end: &mut NativeEnd,
        discipline: &PresentationDiscipline,
    ) -> Result<Option<EndBoundary>> {
        match self {
            Self::Legacy(o) => o.observe_end(end, discipline),
            Self::Complete { .. } => Err("complete Source cannot use scalar end evidence".into()),
        }
    }
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
fn complete_capability(
    output: &WasapiCompleteReplacementOutput,
) -> std::result::Result<
    beatkernel_bms_runtime::gameplay::output::domain::control::OutputCapability,
    String,
> {
    super::output_settings::switch_capability(
        super::output_settings::capability(
            output.stream().configuration(),
            output.channel_matrix(),
        )?,
        Backend::Wasapi,
        None,
    )
}

fn resolve_wasapi_plan(
    plan: super::output_settings::WasapiTargetPlan,
) -> std::result::Result<
    beatkernel_bms_runtime::gameplay::output::domain::remix::RemixedOutputRequest<
        AudioStreamRequest,
    >,
    String,
> {
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
    plan.into_request(default)
}
pub(super) struct WindowsOutputUi {
    bridge: GameplayOutputUi<PlayerOutputUi>,
}
impl WindowsOutputUi {
    pub(super) fn new(owner: &WindowsOutputOwner, enabled: bool) -> Result<Self> {
        let mut bridge = GameplayOutputUi::new(PlayerOutputUi);
        let cap = if enabled {
            match owner {
                WindowsOutputOwner::Legacy(owner) => owner.current().map(capability).transpose()?,
                WindowsOutputOwner::Complete { owner, .. } => {
                    owner.current().map(complete_capability).transpose()?
                }
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
                let request = resolve_wasapi_plan(plan)?;
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
        let WindowsOutputOwner::Legacy(owner) = owner else {
            return Err("complete Source output cannot use scalar publication".into());
        };
        self.bridge
            .service(owner, context, now, &mut Self::map_request, &mut capability)
    }
    pub(super) fn service_audio(
        &mut self,
        owner: &mut WindowsOutputOwner,
        context: GameplayAudioOutputContext<'_>,
        now: ClockPoint,
    ) -> Result<bool> {
        let WindowsOutputOwner::Legacy(owner) = owner else {
            return Err("complete Source output requires coordinate publication".into());
        };
        self.bridge
            .service_audio(owner, context, now, &mut Self::map_request, &mut capability)
    }
    pub(super) fn service_coordinate_source_audio(
        &mut self,
        owner: &mut WindowsOutputOwner,
        context: GameplayAudioOutputContext<'_>,
        now: ClockPoint,
        identity: beatkernel_bms_runtime::gameplay::output::ports::SourcePlaybackIdentity,
        practice_pending: bool,
    ) -> Result<bool> {
        match owner {
            WindowsOutputOwner::Legacy(owner) => {
                // Retained ASIO keeps its existing explicit refusal.
                self.bridge.service_audio(
                    owner,
                    context,
                    now,
                    &mut Self::map_request,
                    &mut capability,
                )
            }
            WindowsOutputOwner::Complete { owner, source } => {
                let source = *source;
                self.bridge.service_coordinate_source_audio(
                    owner,
                    context,
                    now,
                    identity,
                    practice_pending,
                    &mut |request, output| {
                        let plan = super::plan_retained_source_output(
                            &complete_capability(output)?,
                            &output.stream().configuration().requested,
                            source,
                            output.channel_matrix(),
                            &request.args,
                        )?;
                        let resolved = resolve_wasapi_plan(plan)?;
                        beatkernel_platform::audio::validate_known_request_sizing(
                            &resolved.native,
                            AudioLimits::MAX_RENDER_FRAMES,
                        )
                        .map_err(|error| error.to_string())?;
                        Ok(resolved)
                    },
                    &mut complete_capability,
                )
            }
        }
    }
}
