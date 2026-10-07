//! CoreAudio request shape; shared domain/owner retain live replacement policy.
use crate::{
    gameplay::output::{
        adapters::{
            coreaudio::{CoreAudioReplacementBackend, CoreAudioReplacementOutput},
            player::PlayerOutputUi,
            remix::RemixedOutputBackend,
        },
        application::{owner::GameplayOutputOwner, requests::GameplayOutputUi},
        domain::{
            control::OutputCapability,
            remix::{matrix_text, select_matrix, RemixedOutputRequest},
        },
    },
    gameplay_presentation::{GameplayAudioOutputContext, GameplayOutputContext},
    settings::{NativeSettings, SettingsHost},
};
use beatkernel::{
    audio::{AudioFormat, AudioLimits, ChannelMatrix},
    time::{ClockDomainId, ClockPoint},
};
use beatkernel_platform::{
    audio::presentation::discipline::PresentationDiscipline,
    macos::{
        audio::{CoreAudioRequest, CoreAudioStream},
        clock::MachClock,
    },
};

pub type NativeCoreAudioOutputOwner =
    GameplayOutputOwner<RemixedOutputBackend<CoreAudioReplacementBackend>>;
/// Static composition shared by solo and cohort; the caller supplies one clock.
pub fn owner(
    stream: CoreAudioStream,
    clock: MachClock,
    host: ClockDomainId,
) -> NativeCoreAudioOutputOwner {
    NativeCoreAudioOutputOwner::new(
        RemixedOutputBackend::new(CoreAudioReplacementBackend::new(clock, host)),
        CoreAudioReplacementOutput::from_stream(stream),
    )
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
    let applied = output.stream().configuration();
    if applied.format != applied.request.format
        || applied.buffer_frames != applied.request.buffer_frames
        || applied.buffer_frames == 0
        || applied.buffer_frames as usize > AudioLimits::MAX_RENDER_FRAMES
        || output
            .channel_matrix()
            .is_some_and(|matrix| matrix.target_channels() != applied.format.channels())
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
            match output.channel_matrix() {
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
        self.bridge
            .service(owner, context, now, &mut Self::map_request, &mut capability)
    }
    pub fn service_audio(
        &mut self,
        owner: &mut NativeCoreAudioOutputOwner,
        context: GameplayAudioOutputContext<'_>,
        now: ClockPoint,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.bridge
            .service_audio(owner, context, now, &mut Self::map_request, &mut capability)
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
