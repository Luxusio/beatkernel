//! CoreAudio request shape; shared domain/owner retain live replacement policy.
use crate::{
    gameplay::output::{
        adapters::{
            coreaudio::{
                CoreAudioCompleteReplacementBackend, CoreAudioCompleteReplacementOutput,
                CoreAudioReplacementBackend, CoreAudioReplacementOutput,
            },
            player::PlayerOutputUi,
            remix::RemixedOutputBackend,
        },
        application::{owner::GameplayOutputOwner, requests::GameplayOutputUi},
        domain::{
            control::OutputCapability,
            remix::{matrix_text, select_matrix, RemixedOutputRequest},
        },
        ports::{NativeSourcePlaybackQuery, SourcePlaybackIdentity},
    },
    gameplay_presentation::{GameplayAudioOutputContext, GameplayOutputContext},
    live_pause::LivePauseObservation,
    native_audio_presentation::NativeAudioPresentation,
    native_coordinate_control::CoordinateControlFacts,
    native_end::{EndBoundary, NativeEnd},
    native_gameplay::NativeGameplayResult,
    settings::{NativeSettings, SettingsHost},
};
use beatkernel::{
    audio::{
        AudioFormat, AudioLimits, ChannelMatrix, InterpolatedTargetTime, Mixer, OutputFrameBasis,
        RenderReport,
    },
    time::{ClockDomainId, ClockPair, ClockPoint},
};
use beatkernel_platform::{
    audio::{presentation::discipline::PresentationDiscipline, NativeOutputState},
    macos::{
        audio::{
            CoreAudioApplied, CoreAudioRequest, CoreAudioSourcePlaybackSnapshot, CoreAudioStream,
            SourceRetirementTicket,
        },
        clock::MachClock,
    },
};

pub type LegacyOutputOwner = GameplayOutputOwner<RemixedOutputBackend<CoreAudioReplacementBackend>>;
pub type CompleteOutputOwner = GameplayOutputOwner<
    RemixedOutputBackend<CoreAudioCompleteReplacementBackend>,
    NativeOutputState,
    OutputFrameBasis,
    SourceRetirementTicket,
>;
pub enum NativeCoreAudioOutputOwner {
    Legacy(LegacyOutputOwner),
    Complete(CompleteOutputOwner),
}
/// Static composition shared by solo and cohort; the caller supplies one clock.
pub fn owner(
    stream: CoreAudioStream,
    clock: MachClock,
    host: ClockDomainId,
) -> NativeCoreAudioOutputOwner {
    NativeCoreAudioOutputOwner::Legacy(LegacyOutputOwner::new(
        RemixedOutputBackend::new(CoreAudioReplacementBackend::new(clock, host)),
        CoreAudioReplacementOutput::from_stream(stream),
    ))
}
pub fn complete_owner(
    request: CoreAudioRequest,
    mixer: Mixer,
    clock: MachClock,
    host: ClockDomainId,
) -> NativeGameplayResult<NativeCoreAudioOutputOwner> {
    let backend = RemixedOutputBackend::new(CoreAudioCompleteReplacementBackend::new(
        clock,
        host,
        crate::native_audio_startup::SOURCE_COORDINATE_HISTORY_HEADROOM,
    ));
    let owner = CompleteOutputOwner::open_initial_with_ticket(
        backend,
        RemixedOutputRequest {
            native: request,
            matrix: None,
        },
        NativeOutputState::from_mixer(mixer),
        0,
    )?;
    Ok(NativeCoreAudioOutputOwner::Complete(owner))
}
impl NativeCoreAudioOutputOwner {
    pub fn current(&self) -> Option<&CoreAudioReplacementOutput> {
        match self {
            Self::Legacy(owner) => owner.current(),
            Self::Complete(owner) => owner.current().map(|output| output.compatibility_output()),
        }
    }
    pub fn current_mut(&mut self) -> Option<&mut CoreAudioReplacementOutput> {
        match self {
            Self::Legacy(owner) => owner.current_mut(),
            Self::Complete(owner) => owner
                .current_mut()
                .map(|output| output.compatibility_output_mut()),
        }
    }
    pub fn last_issued_epoch(&self) -> u64 {
        match self {
            Self::Legacy(owner) => owner.last_issued_epoch(),
            Self::Complete(owner) => owner.last_issued_epoch(),
        }
    }
    pub fn has_work(&self) -> bool {
        match self {
            Self::Legacy(owner) => owner.has_work(),
            Self::Complete(owner) => owner.has_work(),
        }
    }
    pub fn replacement_pending(&self) -> bool {
        match self {
            Self::Legacy(owner) => owner.replacement_pending(),
            Self::Complete(owner) => owner.replacement_pending(),
        }
    }
    pub fn output_clock_suspended(&self) -> bool {
        match self {
            Self::Legacy(owner) => owner.output_clock_suspended(),
            Self::Complete(owner) => owner.output_clock_suspended(),
        }
    }
    pub fn render_report(&self) -> Option<RenderReport> {
        match self {
            Self::Legacy(owner) => owner.render_report(),
            Self::Complete(owner) => owner.render_report(),
        }
    }
    pub fn stop(&mut self) -> NativeGameplayResult<()> {
        match self {
            Self::Legacy(owner) => Ok(owner.stop()?),
            Self::Complete(owner) => Ok(owner.stop()?),
        }
    }
    pub fn observe_native_control(
        &mut self,
        presentation: &mut NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinateControlFacts>> {
        match self {
            Self::Legacy(owner) => Ok(owner.observe_native_control(presentation, identity)?),
            Self::Complete(owner) => {
                owner.observe_conditional_source_native_control(presentation, identity)
            }
        }
    }
    pub fn source_playback_at(
        &self,
        presentation: &NativeAudioPresentation,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<NativeSourcePlaybackQuery<CoreAudioSourcePlaybackSnapshot>>>
    {
        match self {
            Self::Legacy(owner) => Ok(owner.source_playback_at(presentation, query, identity)?),
            Self::Complete(_) => {
                Err("complete Source query requires the mutable conditional device port".into())
            }
        }
    }
    pub fn conditional_source_playback_at(
        &mut self,
        presentation: &NativeAudioPresentation,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<NativeSourcePlaybackQuery<CoreAudioSourcePlaybackSnapshot>>>
    {
        match self {
            Self::Legacy(owner) => Ok(owner.source_playback_at(presentation, query, identity)?),
            Self::Complete(owner) => {
                owner.conditional_source_playback_at(presentation, query, identity)
            }
        }
    }
    pub fn request_source_history_retirement_with_source_pin(
        &mut self,
        presentation: &NativeAudioPresentation,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
        source_minimum: Option<u64>,
    ) -> NativeGameplayResult<Option<u64>> {
        match self {
            Self::Legacy(owner) => Ok(owner.request_source_history_retirement_with_source_pin(
                presentation,
                query,
                identity,
                source_minimum,
            )?),
            Self::Complete(owner) => owner
                .request_conditional_source_history_retirement_with_source_pin(
                    presentation,
                    query,
                    identity,
                    source_minimum,
                ),
        }
    }
    pub fn observe_native(
        &mut self,
        presentation: &mut NativeAudioPresentation,
    ) -> NativeGameplayResult<()> {
        match self {
            Self::Legacy(owner) => Ok(owner.observe_native(presentation)?),
            Self::Complete(_) => {
                Err("complete Source observation requires coordinate identity".into())
            }
        }
    }
    pub fn audio_pause_observation(
        &mut self,
        presentation: &NativeAudioPresentation,
        now: ClockPoint,
    ) -> NativeGameplayResult<LivePauseObservation> {
        match self {
            Self::Legacy(owner) => Ok(owner.audio_pause_observation(presentation, now)?),
            Self::Complete(_) => {
                Err("complete Source pause requires coordinate control facts".into())
            }
        }
    }
    pub fn observe_audio_end(
        &mut self,
        end: &mut NativeEnd,
        presentation: &NativeAudioPresentation,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        match self {
            Self::Legacy(owner) => owner.observe_audio_end(end, presentation),
            Self::Complete(_) => {
                Err("complete Source end uses retained practice end intent".into())
            }
        }
    }
    pub fn observe(&mut self, discipline: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
        match self {
            Self::Legacy(owner) => Ok(owner.observe(discipline)?),
            Self::Complete(_) => Err("complete Source cannot use scalar presentation".into()),
        }
    }
    pub fn pause_observation(
        &mut self,
        pair: ClockPair,
        now: ClockPoint,
    ) -> NativeGameplayResult<LivePauseObservation> {
        match self {
            Self::Legacy(owner) => Ok(owner.pause_observation(pair, now)?),
            Self::Complete(_) => Err("complete Source cannot use scalar pause evidence".into()),
        }
    }
    pub fn seed_resume(
        &mut self,
        discipline: &mut PresentationDiscipline,
    ) -> NativeGameplayResult<()> {
        match self {
            Self::Legacy(owner) => Ok(owner.seed_resume(discipline)?),
            Self::Complete(_) => Err("complete Source resume requires coordinate adoption".into()),
        }
    }
    pub fn observe_end(
        &mut self,
        end: &mut NativeEnd,
        discipline: &PresentationDiscipline,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        match self {
            Self::Legacy(owner) => owner.observe_end(end, discipline),
            Self::Complete(_) => Err("complete Source cannot use scalar end evidence".into()),
        }
    }
}
pub fn request_for_args(
    current: CoreAudioRequest,
    matrix: Option<&ChannelMatrix>,
    args: &[String],
) -> Result<RemixedOutputRequest<CoreAudioRequest>, String> {
    let draft = NativeSettings::output_only(args, SettingsHost::Macos)?;
    let mut request = current;
    let mut text = "";
    for field in draft.fields() {
        if field.value.is_empty() {
            continue;
        }
        match field.flag {
            "--device" => request.device = positive(&field.value)?,
            "--buffer-frames" => request.buffer_frames = positive(&field.value)?,
            "--output-matrix" => text = &field.value,
            _ => return Err("unsupported CoreAudio output field".into()),
        }
    }
    if request.buffer_frames as usize > AudioLimits::MAX_RENDER_FRAMES {
        return Err("CoreAudio buffer exceeds render limit".into());
    }
    let source = matrix.map_or(current.format.channels(), ChannelMatrix::source_channels);
    let (channels, matrix) = select_matrix(source, matrix, text)?;
    request.format =
        AudioFormat::new(current.format.sample_rate(), channels).map_err(|e| e.to_string())?;
    Ok(RemixedOutputRequest {
        native: request,
        matrix,
    })
}
fn positive(value: &str) -> Result<u32, String> {
    if !value.bytes().all(|c| c.is_ascii_digit()) {
        return Err("CoreAudio device/buffer requires positive decimal u32".into());
    }
    value
        .parse::<u32>()
        .ok()
        .filter(|value| *value != 0)
        .ok_or_else(|| "CoreAudio device/buffer requires positive decimal u32".into())
}
pub fn capability(output: &CoreAudioReplacementOutput) -> Result<OutputCapability, String> {
    capability_for_configuration(
        output.stream().configuration(),
        output.stream().max_callback_frames(),
        output.channel_matrix(),
    )
}
pub(super) fn capability_for_configuration(
    applied: &CoreAudioApplied,
    max_callback_frames: u32,
    matrix: Option<&ChannelMatrix>,
) -> Result<OutputCapability, String> {
    if applied.format != applied.request.format
        || applied.buffer_frames != applied.request.buffer_frames
        || applied.buffer_frames == 0
        || max_callback_frames < applied.buffer_frames
        || max_callback_frames as usize > AudioLimits::MAX_RENDER_FRAMES
        || matrix.is_some_and(|matrix| matrix.target_channels() != applied.format.channels())
    {
        return Err("CoreAudio applied output metadata is inconsistent".into());
    }
    let cap = OutputCapability {
        host: SettingsHost::Macos,
        current_args: vec![
            "--device".into(),
            applied.request.device.to_string(),
            "--buffer-frames".into(),
            applied.buffer_frames.to_string(),
            "--output-matrix".into(),
            match matrix {
                Some(matrix) => matrix_text(matrix)?,
                None => "exact".into(),
            },
        ],
    };
    cap.validate()?;
    Ok(cap)
}
pub struct NativeCoreAudioOutputUi {
    bridge: GameplayOutputUi<PlayerOutputUi>,
}
impl NativeCoreAudioOutputUi {
    pub fn new(
        owner: &NativeCoreAudioOutputOwner,
        enabled: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut bridge = GameplayOutputUi::new(PlayerOutputUi);
        bridge.advertise(if enabled {
            owner.current().map(capability).transpose()?
        } else {
            None
        })?;
        Ok(Self { bridge })
    }
    pub fn pending(&self) -> bool {
        self.bridge.pending()
    }
    fn map_request(
        request: &crate::gameplay::output::domain::control::OutputRequest,
        output: &CoreAudioReplacementOutput,
    ) -> Result<RemixedOutputRequest<CoreAudioRequest>, String> {
        request_for_args(
            output.stream().configuration().request,
            output.channel_matrix(),
            &request.args,
        )
    }

    pub fn service(
        &mut self,
        owner: &mut NativeCoreAudioOutputOwner,
        context: GameplayOutputContext<'_, PresentationDiscipline>,
        now: ClockPoint,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let NativeCoreAudioOutputOwner::Legacy(owner) = owner else {
            return Err("complete Source output cannot use scalar publication".into());
        };
        self.bridge
            .service(owner, context, now, &mut Self::map_request, &mut capability)
    }
    pub fn service_audio(
        &mut self,
        owner: &mut NativeCoreAudioOutputOwner,
        context: GameplayAudioOutputContext<'_>,
        now: ClockPoint,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let NativeCoreAudioOutputOwner::Legacy(owner) = owner else {
            return Err("complete Source output requires coordinate publication".into());
        };
        self.bridge
            .service_audio(owner, context, now, &mut Self::map_request, &mut capability)
    }
    pub fn service_coordinate_source_audio(
        &mut self,
        owner: &mut NativeCoreAudioOutputOwner,
        context: GameplayAudioOutputContext<'_>,
        now: ClockPoint,
        identity: SourcePlaybackIdentity,
        practice_pending: bool,
    ) -> NativeGameplayResult<bool> {
        match owner {
            NativeCoreAudioOutputOwner::Legacy(owner) => self.bridge.service_audio(
                owner,
                context,
                now,
                &mut Self::map_request,
                &mut capability,
            ),
            NativeCoreAudioOutputOwner::Complete(owner) => {
                self.bridge.service_coordinate_source_audio(
                    owner,
                    context,
                    now,
                    identity,
                    practice_pending,
                    &mut |request, output: &CoreAudioCompleteReplacementOutput| {
                        Self::map_request(request, output.compatibility_output())
                    },
                    &mut |output: &CoreAudioCompleteReplacementOutput| {
                        capability(output.compatibility_output())
                    },
                )
            }
        }
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    #[test]
    fn typed_coreaudio_output_mapping_preserves_rate_rejects_width_and_resets_source() {
        let current = CoreAudioRequest {
            device: 1,
            buffer_frames: 64,
            format: AudioFormat::new(48_000, 1).unwrap(),
        };
        let request =
            request_for_args(current, None, &["--output-matrix".into(), "1;0.5".into()]).unwrap();
        assert_eq!(request.native.format.channels(), 2);
        let buffer = request_for_args(
            request.native,
            request.matrix.as_ref(),
            &["--buffer-frames".into(), "32".into()],
        )
        .unwrap();
        assert_eq!(buffer.matrix, request.matrix);
        assert_eq!(buffer.native.format.sample_rate(), 48_000);
        assert!(request_for_args(
            request.native,
            request.matrix.as_ref(),
            &["--output-matrix".into(), "1,0".into()]
        )
        .is_err());
        let exact = request_for_args(
            request.native,
            request.matrix.as_ref(),
            &["--output-matrix".into(), "exact".into()],
        )
        .unwrap();
        assert_eq!(exact.native.format.channels(), 1);
        assert!(exact.matrix.is_none());
        assert!(request_for_args(current, None, &["--device".into(), "0".into()]).is_err());
    }
}

#[cfg(test)]
#[path = "coreaudio_ui_fixtures.rs"]
mod complete_fixtures;
