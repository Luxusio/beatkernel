//! WASAPI lifecycle adapter; all observed counters retain their creation epoch.
use crate::{
    gameplay::output::adapters::observation::{observe_wasapi, ReplacementObservationError},
    gameplay::output::ports::OutputReplacementBackend,
};
use beatkernel::audio::{
    Mixer, MixerOpenFailure, OutputFrameBasis, OutputOpenFailure, RenderReport, StoppedMixerSource,
};
use beatkernel_platform::{
    audio::{
        presentation::discipline::PresentationDiscipline, AudioOutputStream, AudioPlatformError,
        AudioStreamRequest,
    },
    windows::{
        audio::{WasapiBackend, WasapiOptions, WasapiStream},
        clock::QpcClock,
    },
};
#[derive(Debug)]
pub enum WasapiReplacementError {
    Playback(crate::gameplay::output::ports::SourcePlaybackPacketError),
    Coordinate(crate::native_output_coordinate::NativeOutputCoordinateError),
    Native(AudioPlatformError),
    Observation(ReplacementObservationError),
}
impl std::fmt::Display for WasapiReplacementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Playback(error) => error.fmt(f),
            Self::Coordinate(error) => error.fmt(f),
            Self::Native(e) => e.fmt(f),
            Self::Observation(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for WasapiReplacementError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Playback(error) => Some(error),
            Self::Coordinate(error) => Some(error),
            Self::Native(e) => Some(e),
            Self::Observation(e) => Some(e),
        }
    }
}
pub struct WasapiReplacementOutput {
    stream: WasapiStream,
    epoch: u64,
}
impl WasapiReplacementOutput {
    pub fn from_stream(stream: WasapiStream) -> Self {
        Self { stream, epoch: 0 }
    }
    pub fn stream(&self) -> &WasapiStream {
        &self.stream
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}
impl StoppedMixerSource for WasapiReplacementOutput {
    type Error = WasapiReplacementError;
    fn take_stopped_mixer(&mut self) -> Result<Option<Mixer>, Self::Error> {
        self.stream
            .take_stopped_mixer()
            .map_err(WasapiReplacementError::Native)
    }
}
fn map_open_failure(
    failure: MixerOpenFailure<AudioPlatformError>,
) -> OutputOpenFailure<WasapiReplacementError, WasapiReplacementOutput> {
    let (error, mixer) = failure.into_parts();
    OutputOpenFailure::recovered(WasapiReplacementError::Native(error), mixer)
}
pub struct WasapiReplacementBackend {
    clock: QpcClock,
    options: WasapiOptions,
}
impl WasapiReplacementBackend {
    pub fn new(clock: QpcClock, options: WasapiOptions) -> Self {
        Self { clock, options }
    }
}
impl OutputReplacementBackend for WasapiReplacementBackend {
    type Presentation = PresentationDiscipline;
    type Output = WasapiReplacementOutput;
    type Request = AudioStreamRequest;
    type Error = WasapiReplacementError;
    fn open(
        &mut self,
        request: Self::Request,
        mixer: Mixer,
        epoch: u64,
    ) -> Result<Self::Output, OutputOpenFailure<Self::Error, Self::Output>> {
        WasapiBackend
            .open_recoverable(request, mixer, self.clock, self.options)
            .map(|stream| WasapiReplacementOutput { stream, epoch })
            .map_err(map_open_failure)
    }
    fn retire(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        output.stream.stop().map_err(WasapiReplacementError::Native)
    }
    fn start(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        output
            .stream
            .start()
            .map_err(WasapiReplacementError::Native)
    }
    fn epoch(&self, output: &Self::Output) -> u64 {
        output.epoch
    }
    fn basis(&self, output: &Self::Output) -> OutputFrameBasis {
        output.stream.frame_basis()
    }
    fn observe(
        &mut self,
        output: &mut Self::Output,
        presentation: &mut Self::Presentation,
    ) -> Result<(), Self::Error> {
        observe_wasapi(
            presentation,
            output.epoch,
            output.stream.snapshot(),
            output.stream.frame_basis(),
        )
        .map(|_| ())
        .map_err(WasapiReplacementError::Observation)
    }
    fn render_report(&self, output: &Self::Output) -> Result<Option<RenderReport>, Self::Error> {
        Ok(output.stream.snapshot().render)
    }
}

impl crate::gameplay::output::ports::OutputChannelRemixBackend for WasapiReplacementBackend {
    fn open_remixed(
        &mut self,
        request: AudioStreamRequest,
        mixer: Mixer,
        epoch: u64,
        matrix: beatkernel::audio::ChannelMatrix,
    ) -> Result<
        WasapiReplacementOutput,
        OutputOpenFailure<WasapiReplacementError, WasapiReplacementOutput>,
    > {
        WasapiBackend
            .open_remixed_recoverable(request, mixer, self.clock, self.options, matrix)
            .map(|stream| WasapiReplacementOutput { stream, epoch })
            .map_err(map_open_failure)
    }
}
#[cfg(test)]
#[path = "wasapi_fixtures.rs"]
mod fixtures;

impl WasapiReplacementBackend {
    /// Observes the existing stream without transferring its creation epoch or ownership.
    pub fn observe_stream_native(
        stream: &WasapiStream,
        epoch: u64,
    ) -> Result<Option<crate::native_audio_presentation::NativeAudioSnapshot>, WasapiReplacementError>
    {
        use beatkernel_platform::audio::{
            presentation::{observation_with_basis, PresentationError},
            AudioStreamStatus,
        };
        let snapshot = stream.snapshot();
        match snapshot.status {
            AudioStreamStatus::Ready => return Ok(None),
            AudioStreamStatus::Running => {}
            status => {
                return Err(WasapiReplacementError::Observation(
                    ReplacementObservationError::Status(status),
                ));
            }
        }
        let basis = stream.frame_basis();
        if matches!(
            observation_with_basis(snapshot, basis),
            Err(PresentationError::Unavailable | PresentationError::BeforePresentation)
        ) {
            return Ok(None);
        }
        Ok(Some(crate::native_audio_presentation::NativeAudioSnapshot {
            epoch, basis,
            evidence: beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence::Wasapi { snapshot, basis: Some(basis) },
        }))
    }

    /// Preserves the native counter and its actual frequency from the same observation.
    pub fn observe_stream_coordinate(
        stream: &WasapiStream,
        epoch: u64,
    ) -> Result<
        Option<crate::native_output_coordinate::CoordinateNativeAudioSnapshot>,
        WasapiReplacementError,
    > {
        use crate::native_output_coordinate::{
            CoordinateNativeAudioSnapshot, NativeOutputCoordinate, NativeOutputCoordinateError,
        };
        Self::observe_stream_native(stream, epoch)?.map(|snapshot| {
            let beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence::Wasapi { snapshot: native, .. } = snapshot.evidence
            else { return Err(WasapiReplacementError::Coordinate(NativeOutputCoordinateError::UnsupportedEvidence)); };
            let clock = native.clock.ok_or(WasapiReplacementError::Coordinate(NativeOutputCoordinateError::MalformedEvidence))?;
            CoordinateNativeAudioSnapshot::new(snapshot, NativeOutputCoordinate::Counter {
                position: clock.position, frequency: clock.frequency,
            }).map_err(WasapiReplacementError::Coordinate)
        }).transpose()
    }

    /// Reads a Source query from the original stream history without changing its clock.
    pub fn source_playback_for_stream(
        stream: &WasapiStream,
        epoch: u64,
        query: &beatkernel::audio::InterpolatedTargetTime,
        identity: crate::gameplay::output::ports::SourcePlaybackIdentity,
    ) -> Result<
        Option<
            crate::gameplay::output::ports::NativeSourcePlaybackQuery<
                beatkernel_platform::audio::AudioStreamSnapshot,
            >,
        >,
        WasapiReplacementError,
    > {
        let basis = stream.frame_basis();
        if basis.sample_rate() != identity.hz()
            || identity.initial_physical() > basis.start_physical_frame()
        {
            return Err(WasapiReplacementError::Playback(
                crate::gameplay::output::ports::SourcePlaybackPacketError::BasisMismatch,
            ));
        }
        stream
            .source_playback_at(
                query,
                identity.hz(),
                identity.initial_physical(),
                identity.initial_playback(),
            )
            .map(|snapshot| {
                crate::gameplay::output::adapters::observation::qualify_wasapi_source_packet(
                    epoch, basis, identity, *query, snapshot,
                )
                .map_err(WasapiReplacementError::Playback)
            })
            .transpose()
    }

    /// Qualifies playback only against a matching original native publication.
    pub fn observe_stream_playback(
        stream: &WasapiStream,
        epoch: u64,
        identity: crate::gameplay::output::ports::SourcePlaybackIdentity,
    ) -> Result<
        Option<
            crate::gameplay::output::ports::NativePlaybackObservation<
                beatkernel_platform::audio::AudioStreamSnapshot,
            >,
        >,
        WasapiReplacementError,
    > {
        for _ in 0..3 {
            let Some(candidate) = Self::observe_stream_coordinate(stream, epoch)? else {
                continue;
            };
            let query = candidate
                .source_query()
                .map_err(WasapiReplacementError::Coordinate)?;
            let Some(packet) = Self::source_playback_for_stream(stream, epoch, &query, identity)?
            else {
                continue;
            };
            if let Some(observation) = crate::gameplay::output::adapters::observation::qualify_wasapi_source_observation_packet(
                candidate, identity, packet,
            ).map_err(WasapiReplacementError::Playback)? {
                return Ok(Some(observation));
            }
        }
        Ok(None)
    }
}

impl crate::gameplay::output::ports::OriginalNativeOutputBackend for WasapiReplacementBackend {
    fn observe_native(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<Option<crate::native_audio_presentation::NativeAudioSnapshot>, Self::Error> {
        Self::observe_stream_native(&output.stream, output.epoch)
    }
}

impl crate::gameplay::output::ports::OriginalCoordinateNativeOutputBackend
    for WasapiReplacementBackend
{
    fn observe_native_coordinate(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<Option<crate::native_output_coordinate::CoordinateNativeAudioSnapshot>, Self::Error>
    {
        Self::observe_stream_coordinate(&output.stream, output.epoch)
    }
}

impl crate::gameplay::output::ports::SourcePlaybackNativeOutputBackend
    for WasapiReplacementBackend
{
    type NativeEvidence = beatkernel_platform::audio::AudioStreamSnapshot;
    fn source_playback_at(
        &self,
        output: &Self::Output,
        query: &beatkernel::audio::InterpolatedTargetTime,
        identity: crate::gameplay::output::ports::SourcePlaybackIdentity,
    ) -> Result<
        Option<crate::gameplay::output::ports::NativeSourcePlaybackQuery<Self::NativeEvidence>>,
        Self::Error,
    > {
        Self::source_playback_for_stream(&output.stream, output.epoch, query, identity)
    }
    fn observe_native_playback(
        &mut self,
        output: &mut Self::Output,
        identity: crate::gameplay::output::ports::SourcePlaybackIdentity,
    ) -> Result<
        Option<crate::gameplay::output::ports::NativePlaybackObservation<Self::NativeEvidence>>,
        Self::Error,
    > {
        Self::observe_stream_playback(&output.stream, output.epoch, identity)
    }
}

impl crate::gameplay::output::ports::SourceHistoryRetirementNativeOutputBackend
    for WasapiReplacementBackend
{
    fn request_source_history_retirement(
        &mut self,
        output: &mut Self::Output,
        expected_epoch: u64,
        expected_basis: OutputFrameBasis,
        identity: crate::gameplay::output::ports::SourcePlaybackIdentity,
        source_pin: u64,
    ) -> Result<Option<u64>, Self::Error> {
        use crate::gameplay::output::ports::SourcePlaybackPacketError;
        if output.epoch != expected_epoch {
            return Err(WasapiReplacementError::Playback(
                SourcePlaybackPacketError::EpochMismatch,
            ));
        }
        if output.stream.frame_basis() != expected_basis {
            return Err(WasapiReplacementError::Playback(
                SourcePlaybackPacketError::BasisMismatch,
            ));
        }
        output
            .stream
            .request_source_history_retirement(
                expected_basis,
                (
                    identity.hz(),
                    identity.initial_physical(),
                    identity.initial_playback(),
                ),
                source_pin,
            )
            .map_err(WasapiReplacementError::Native)
    }
}
