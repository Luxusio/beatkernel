//! CoreAudio replacement retains actual callback owners and thread affinity.
use crate::gameplay::output::ports::OutputReplacementBackend;
use beatkernel::{
    audio::{Mixer, OutputFrameBasis, OutputOpenFailure, RenderReport, StoppedMixerSource},
    time::ClockDomainId,
};
use beatkernel_platform::{
    audio::presentation::discipline::{DisciplineError, PresentationDiscipline},
    macos::{
        audio::{CoreAudioError, CoreAudioOpenFailure, CoreAudioRequest, CoreAudioStream},
        clock::MachClock,
        presentation::{coreaudio_presentation_pair, CoreAudioPresentationError},
    },
};
#[derive(Debug)]
pub enum CoreAudioReplacementError {
    Coordinate(crate::native_output_coordinate::NativeOutputCoordinateError),
    Playback(crate::gameplay::output::ports::SourcePlaybackPacketError),
    Native(CoreAudioError),
    Presentation(CoreAudioPresentationError),
    Discipline(DisciplineError),
    CallbackFailure(u64),
    EpochMismatch,
}
impl std::fmt::Display for CoreAudioReplacementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Coordinate(error) => error.fmt(f),
            Self::Playback(error) => error.fmt(f),
            Self::Native(e) => e.fmt(f),
            Self::Presentation(e) => e.fmt(f),
            Self::Discipline(e) => e.fmt(f),
            Self::CallbackFailure(count) => {
                write!(f, "CoreAudio replacement callback failures: {count}")
            }
            Self::EpochMismatch => f.write_str("CoreAudio replacement observation epoch differs"),
        }
    }
}
impl std::error::Error for CoreAudioReplacementError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Coordinate(error) => Some(error),
            Self::Playback(error) => Some(error),
            Self::Native(e) => Some(e),
            Self::Presentation(e) => Some(e),
            Self::Discipline(e) => Some(e),
            _ => None,
        }
    }
}
pub struct CoreAudioReplacementOutput {
    stream: CoreAudioStream,
    epoch: u64,
    matrix: Option<beatkernel::audio::ChannelMatrix>,
}
impl CoreAudioReplacementOutput {
    pub fn from_stream(stream: CoreAudioStream) -> Self {
        Self {
            stream,
            epoch: 0,
            matrix: None,
        }
    }
    pub fn stream(&self) -> &CoreAudioStream {
        &self.stream
    }
    pub fn stream_mut(&mut self) -> &mut CoreAudioStream {
        &mut self.stream
    }
    pub fn channel_matrix(&self) -> Option<&beatkernel::audio::ChannelMatrix> {
        self.matrix.as_ref()
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}
impl StoppedMixerSource for CoreAudioReplacementOutput {
    type Error = CoreAudioReplacementError;
    fn take_stopped_mixer(&mut self) -> Result<Option<Mixer>, Self::Error> {
        self.stream
            .take_stopped_mixer()
            .map_err(CoreAudioReplacementError::Native)
    }
}
fn map_open_failure(
    failure: CoreAudioOpenFailure,
    epoch: u64,
) -> OutputOpenFailure<CoreAudioReplacementError, CoreAudioReplacementOutput> {
    let (error, mixer, pending, cleanup) = failure.into_parts();
    let mapped = match pending {
        Some(stream) => OutputOpenFailure::pending(
            CoreAudioReplacementError::Native(error),
            CoreAudioReplacementOutput {
                stream,
                epoch,
                matrix: None,
            },
        ),
        None => OutputOpenFailure::recovered(CoreAudioReplacementError::Native(error), mixer),
    };
    match cleanup {
        Some(error) => mapped.with_cleanup_error(CoreAudioReplacementError::Native(error)),
        None => mapped,
    }
}
pub struct CoreAudioReplacementBackend {
    clock: MachClock,
    host_domain: ClockDomainId,
}
impl CoreAudioReplacementBackend {
    pub fn new(clock: MachClock, host_domain: ClockDomainId) -> Self {
        Self { clock, host_domain }
    }
}
impl OutputReplacementBackend for CoreAudioReplacementBackend {
    type Presentation = PresentationDiscipline;
    type Output = CoreAudioReplacementOutput;
    type Request = CoreAudioRequest;
    type Error = CoreAudioReplacementError;
    fn open(
        &mut self,
        request: Self::Request,
        mixer: Mixer,
        epoch: u64,
    ) -> Result<Self::Output, OutputOpenFailure<Self::Error, Self::Output>> {
        CoreAudioStream::open_recoverable(request, self.clock, mixer)
            .map(|stream| CoreAudioReplacementOutput {
                stream,
                epoch,
                matrix: None,
            })
            .map_err(|error| map_open_failure(error, epoch))
    }
    fn retire(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        output
            .stream
            .stop()
            .map_err(CoreAudioReplacementError::Native)?;
        Ok(())
    }
    fn start(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        output
            .stream
            .start()
            .map_err(CoreAudioReplacementError::Native)?;
        Ok(())
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
        if presentation.epoch() != output.epoch {
            return Err(CoreAudioReplacementError::EpochMismatch);
        }
        if let Some(pair) = self.original_pair(output)? {
            presentation
                .observe_clock_pair_in_epoch(output.epoch, pair)
                .map_err(CoreAudioReplacementError::Discipline)?;
        }
        Ok(())
    }
    fn render_report(&self, output: &Self::Output) -> Result<Option<RenderReport>, Self::Error> {
        Ok(output.stream.last_render_report())
    }
}

impl crate::gameplay::output::ports::OutputChannelRemixBackend for CoreAudioReplacementBackend {
    fn open_remixed(
        &mut self,
        request: CoreAudioRequest,
        mixer: Mixer,
        epoch: u64,
        matrix: beatkernel::audio::ChannelMatrix,
    ) -> Result<
        CoreAudioReplacementOutput,
        OutputOpenFailure<CoreAudioReplacementError, CoreAudioReplacementOutput>,
    > {
        let native_matrix = match beatkernel::audio::ChannelMatrix::new(
            matrix.source_channels(),
            matrix.target_channels(),
            matrix.coefficients(),
        ) {
            Ok(copy) => copy,
            Err(_) => {
                return Err(OutputOpenFailure::recovered(
                    CoreAudioReplacementError::Native(CoreAudioError::Capacity),
                    Some(mixer),
                ));
            }
        };
        CoreAudioStream::open_remixed_recoverable(request, self.clock, mixer, native_matrix)
            .map(|stream| CoreAudioReplacementOutput {
                stream,
                epoch,
                matrix: Some(matrix),
            })
            .map_err(|failure| map_open_failure(failure, epoch))
    }
}
#[cfg(test)]
#[path = "coreaudio_fixtures.rs"]
mod fixtures;

impl CoreAudioReplacementBackend {
    fn original_pair(
        &mut self,
        output: &mut CoreAudioReplacementOutput,
    ) -> Result<Option<beatkernel::time::ClockPair>, CoreAudioReplacementError> {
        Ok(self.original_coordinate(output)?.map(|(pair, _)| pair))
    }
    fn original_coordinate(
        &mut self,
        output: &mut CoreAudioReplacementOutput,
    ) -> Result<Option<(beatkernel::time::ClockPair, u64)>, CoreAudioReplacementError> {
        let snapshot = output.stream.snapshot();
        if snapshot.configuration_changed {
            return Err(CoreAudioReplacementError::Native(
                CoreAudioError::ConfigurationChanged,
            ));
        }
        if snapshot.callback_failures != 0 {
            return Err(CoreAudioReplacementError::CallbackFailure(
                snapshot.callback_failures,
            ));
        }
        if !output.stream.is_started() {
            return Ok(None);
        }
        let Some(observation) = snapshot.presentation else {
            return Ok(None);
        };
        coreaudio_presentation_pair(
            observation,
            output.stream.configuration(),
            self.host_domain,
            &self.clock,
        )
        .map(|pair| pair.map(|pair| (pair, observation.first_frame)))
        .map_err(CoreAudioReplacementError::Presentation)
    }
}

impl crate::gameplay::output::ports::OriginalNativeOutputBackend for CoreAudioReplacementBackend {
    fn observe_native(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<Option<crate::native_audio_presentation::NativeAudioSnapshot>, Self::Error> {
        Ok(self.original_pair(output)?.map(|pair| crate::native_audio_presentation::NativeAudioSnapshot {
            epoch: output.epoch, basis: output.stream.frame_basis(),
            evidence: beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence::SuppliedPair(pair),
        }))
    }
}

impl crate::gameplay::output::ports::OriginalCoordinateNativeOutputBackend
    for CoreAudioReplacementBackend
{
    fn observe_native_coordinate(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<Option<crate::native_output_coordinate::CoordinateNativeAudioSnapshot>, Self::Error>
    {
        use crate::native_output_coordinate::{
            CoordinateNativeAudioSnapshot, NativeOutputCoordinate, NativeOutputCoordinateError,
        };
        self.original_coordinate(output)?.map(|(pair, first_frame)| {
            let basis = output.stream.frame_basis();
            let frame = first_frame.checked_sub(basis.start_physical_frame())
                .ok_or(CoreAudioReplacementError::Coordinate(NativeOutputCoordinateError::FrameBeforeBasis))?;
            CoordinateNativeAudioSnapshot::new(
                crate::native_audio_presentation::NativeAudioSnapshot {
                    epoch: output.epoch, basis,
                    evidence: beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence::SuppliedPair(pair),
                }, NativeOutputCoordinate::StreamFrame { frame },
            ).map_err(CoreAudioReplacementError::Coordinate)
        }).transpose()
    }
}

use crate::gameplay::output::ports::{
    NativePlaybackObservation, NativeSourcePlaybackQuery, SourcePlaybackFacts,
    SourcePlaybackIdentity, SourcePlaybackNativeOutputBackend, SourcePlaybackPacketError,
};
use crate::native_output_coordinate::{
    CoordinateNativeAudioSnapshot, NativeOutputCoordinate, NativeOutputCoordinateError,
};
use beatkernel::audio::InterpolatedTargetTime;
use beatkernel_platform::macos::audio::{CoreAudioSnapshot, CoreAudioSourcePlaybackSnapshot};

fn validate_source_callback(snapshot: CoreAudioSnapshot) -> Result<(), CoreAudioReplacementError> {
    if snapshot.configuration_changed {
        return Err(CoreAudioReplacementError::Native(
            CoreAudioError::ConfigurationChanged,
        ));
    }
    if snapshot.callback_failures != 0 {
        return Err(CoreAudioReplacementError::CallbackFailure(
            snapshot.callback_failures,
        ));
    }
    Ok(())
}

/// Successful delivery is independent of source generation. This packet retains
/// the complete coherent callback evidence without converting or admitting HOST.
pub(super) fn qualify_coreaudio_source_packet(
    epoch: u64,
    basis: OutputFrameBasis,
    identity: SourcePlaybackIdentity,
    query: InterpolatedTargetTime,
    snapshot: CoreAudioSourcePlaybackSnapshot,
) -> Result<NativeSourcePlaybackQuery<CoreAudioSourcePlaybackSnapshot>, CoreAudioReplacementError> {
    validate_source_callback(snapshot.snapshot)?;
    if snapshot.revision == 0 || snapshot.revision & 1 != 0 {
        return Err(CoreAudioReplacementError::Playback(
            SourcePlaybackPacketError::InvalidRevision,
        ));
    }
    let presentation =
        snapshot
            .snapshot
            .presentation
            .ok_or(CoreAudioReplacementError::Playback(
                SourcePlaybackPacketError::MissingNativeEvidence,
            ))?;
    if presentation.frames == 0
        || presentation.first_frame < basis.start_physical_frame()
        || presentation
            .first_frame
            .checked_add(u64::from(presentation.frames))
            != Some(snapshot.delivered_end_frame)
    {
        return Err(CoreAudioReplacementError::Playback(
            SourcePlaybackPacketError::InvalidPrefix,
        ));
    }
    NativeSourcePlaybackQuery::new(
        epoch,
        basis,
        identity,
        query,
        SourcePlaybackFacts {
            playback: snapshot.playback,
            pulled_source_end: snapshot.pulled_source_end,
            submitted_source_end: snapshot.delivered_end_frame,
            render: snapshot.render,
            revision: snapshot.revision,
        },
        Some(snapshot),
    )
    .map_err(CoreAudioReplacementError::Playback)
}

/// A second coherent read must still contain the original native callback. A
/// changed publication waits; mismatched owner headers or coordinates refuse.
pub(super) fn qualify_coreaudio_observation_packet(
    candidate: CoordinateNativeAudioSnapshot,
    native: CoreAudioSnapshot,
    identity: SourcePlaybackIdentity,
    packet: NativeSourcePlaybackQuery<CoreAudioSourcePlaybackSnapshot>,
) -> Result<
    Option<NativePlaybackObservation<CoreAudioSourcePlaybackSnapshot>>,
    CoreAudioReplacementError,
> {
    validate_source_callback(native)?;
    let snapshot = candidate.snapshot();
    if packet.epoch() != snapshot.epoch {
        return Err(CoreAudioReplacementError::Playback(
            SourcePlaybackPacketError::EpochMismatch,
        ));
    }
    if packet.basis() != snapshot.basis || packet.identity() != identity {
        return Err(CoreAudioReplacementError::Playback(
            SourcePlaybackPacketError::BasisMismatch,
        ));
    }
    let Some(evidence) = packet.native_evidence() else {
        return Err(CoreAudioReplacementError::Playback(
            SourcePlaybackPacketError::MissingNativeEvidence,
        ));
    };
    if evidence.snapshot != native {
        return Ok(None);
    }
    let presentation = native
        .presentation
        .ok_or(CoreAudioReplacementError::Playback(
            SourcePlaybackPacketError::MissingNativeEvidence,
        ))?;
    let NativeOutputCoordinate::StreamFrame { frame } = candidate.coordinate() else {
        return Err(CoreAudioReplacementError::Playback(
            SourcePlaybackPacketError::CoordinateMismatch,
        ));
    };
    if snapshot.basis.start_physical_frame().checked_add(frame) != Some(presentation.first_frame) {
        return Err(CoreAudioReplacementError::Playback(
            SourcePlaybackPacketError::CoordinateMismatch,
        ));
    }
    NativePlaybackObservation::new(candidate, packet)
        .map(Some)
        .map_err(CoreAudioReplacementError::Playback)
}

impl SourcePlaybackNativeOutputBackend for CoreAudioReplacementBackend {
    type NativeEvidence = CoreAudioSourcePlaybackSnapshot;

    fn source_playback_at(
        &self,
        output: &Self::Output,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativeSourcePlaybackQuery<Self::NativeEvidence>>, Self::Error> {
        let basis = output.stream.frame_basis();
        if basis.sample_rate() != identity.hz()
            || identity.initial_physical() > basis.start_physical_frame()
        {
            return Err(CoreAudioReplacementError::Playback(
                SourcePlaybackPacketError::BasisMismatch,
            ));
        }
        validate_source_callback(output.stream.snapshot())?;
        if !output.stream.is_started() {
            return Ok(None);
        }
        output
            .stream
            .source_playback_at(
                query,
                identity.hz(),
                identity.initial_physical(),
                identity.initial_playback(),
            )
            .map(|snapshot| {
                qualify_coreaudio_source_packet(output.epoch, basis, identity, *query, snapshot)
            })
            .transpose()
    }

    fn observe_native_playback(
        &mut self,
        output: &mut Self::Output,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativePlaybackObservation<Self::NativeEvidence>>, Self::Error> {
        let basis = output.stream.frame_basis();
        if basis.sample_rate() != identity.hz()
            || identity.initial_physical() > basis.start_physical_frame()
        {
            return Err(CoreAudioReplacementError::Playback(
                SourcePlaybackPacketError::BasisMismatch,
            ));
        }
        for _ in 0..3 {
            let native = output.stream.snapshot();
            validate_source_callback(native)?;
            if !output.stream.is_started() {
                return Ok(None);
            }
            let Some(presentation) = native.presentation else {
                continue;
            };
            let Some(pair) = coreaudio_presentation_pair(
                presentation,
                output.stream.configuration(),
                self.host_domain,
                &self.clock,
            )
            .map_err(CoreAudioReplacementError::Presentation)?
            else {
                continue;
            };
            let frame = presentation
                .first_frame
                .checked_sub(basis.start_physical_frame())
                .ok_or(CoreAudioReplacementError::Coordinate(
                    NativeOutputCoordinateError::FrameBeforeBasis,
                ))?;
            let candidate = CoordinateNativeAudioSnapshot::new(
                crate::native_audio_presentation::NativeAudioSnapshot {
                    epoch: output.epoch,
                    basis,
                    evidence: beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence::SuppliedPair(pair),
                },
                NativeOutputCoordinate::StreamFrame { frame },
            ).map_err(CoreAudioReplacementError::Coordinate)?;
            let query = candidate
                .source_query()
                .map_err(CoreAudioReplacementError::Coordinate)?;
            let Some(packet) = self.source_playback_at(output, &query, identity)? else {
                continue;
            };
            if let Some(observation) =
                qualify_coreaudio_observation_packet(candidate, native, identity, packet)?
            {
                return Ok(Some(observation));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
#[path = "coreaudio_source_packet_fixtures.rs"]
mod source_packet_fixtures;

impl crate::gameplay::output::ports::SourceHistoryRetirementNativeOutputBackend
    for CoreAudioReplacementBackend
{
    fn request_source_history_retirement(
        &mut self,
        output: &mut Self::Output,
        expected_epoch: u64,
        expected_basis: OutputFrameBasis,
        identity: SourcePlaybackIdentity,
        source_pin: u64,
    ) -> Result<Option<u64>, Self::Error> {
        if output.epoch != expected_epoch {
            return Err(CoreAudioReplacementError::Playback(
                SourcePlaybackPacketError::EpochMismatch,
            ));
        }
        if output.stream.frame_basis() != expected_basis {
            return Err(CoreAudioReplacementError::Playback(
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
            .map_err(CoreAudioReplacementError::Native)
    }
}
