//! ALSA-only output conversion on the native play thread, before retirement.
use crate::{
    gameplay::output::adapters::alsa::{AlsaReplacementBackend, AlsaReplacementOutput},
    gameplay::output::adapters::player::PlayerOutputUi,
    gameplay::output::application::owner::GameplayOutputOwner,
    gameplay::output::application::requests::GameplayOutputUi,
    gameplay::output::domain::control::{OutputCapability, OutputRequest},
    gameplay::output::{
        adapters::remix::RemixedOutputBackend,
        domain::remix::{matrix_text, select_matrix, RemixedOutputRequest},
    },
    gameplay_presentation::{GameplayAudioOutputContext, GameplayOutputContext},
    settings::{NativeSettings, SettingsHost},
};
use beatkernel::{
    audio::{AudioLimits, ChannelMatrix},
    time::ClockPoint,
};
use beatkernel_platform::{
    audio::{presentation::discipline::PresentationDiscipline, DeviceFormat},
    linux::{AlsaAppliedConfig, AlsaRequest},
};
pub fn request_for_args(current: &AlsaRequest, args: &[String]) -> Result<AlsaRequest, String> {
    let draft = NativeSettings::output_only(args, SettingsHost::Linux)?;
    let mut request = current.clone();
    for field in draft.fields() {
        if field.value.is_empty() {
            continue;
        }
        match field.flag {
            "--alsa" => request.device = field.value.clone(),
            "--period-frames" => request.period_frames = parse_frames(&field.value)?,
            "--buffer-frames" => request.buffer_frames = parse_frames(&field.value)?,
            "--output-matrix" => {
                return Err("channel matrices require typed output requests".into());
            }
            _ => return Err("unsupported ALSA output field".into()),
        }
    }
    if request.period_frames == 0
        || request.period_frames >= request.buffer_frames
        || request.period_frames as usize > AudioLimits::MAX_RENDER_FRAMES
    {
        return Err("processing period must be positive, within the render limit and smaller than the buffer".into());
    }
    Ok(request)
}

pub type NativeAlsaOutputOwner = GameplayOutputOwner<RemixedOutputBackend<AlsaReplacementBackend>>;
pub fn remixed_request_for_args(
    current: &AlsaRequest,
    current_matrix: Option<&ChannelMatrix>,
    args: &[String],
) -> Result<RemixedOutputRequest<AlsaRequest>, String> {
    let draft = NativeSettings::output_only(args, SettingsHost::Linux)?;
    let native_args: Vec<String> = draft
        .fields()
        .iter()
        .filter(|field| field.flag != "--output-matrix" && !field.value.is_empty())
        .flat_map(|field| [field.flag.to_owned(), field.value.clone()])
        .collect();
    let mut native = request_for_args(current, &native_args)?;
    let value = draft
        .fields()
        .iter()
        .find(|field| field.flag == "--output-matrix")
        .map_or("", |field| field.value.as_str());
    let source_channels =
        current_matrix.map_or(current.format.channels(), ChannelMatrix::source_channels);
    let (channels, matrix) = select_matrix(source_channels, current_matrix, value)?;
    native.format = DeviceFormat::new(
        current.format.sample_rate(),
        channels,
        current.format.encoding(),
        current.format.channel_mask(),
    )
    .map_err(|e| e.to_string())?;
    Ok(RemixedOutputRequest { native, matrix })
}
fn parse_frames(value: &str) -> Result<u32, String> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("frame count must be unsigned decimal".into());
    }
    value.parse().map_err(|_| "frame count exceeds u32".into())
}
pub fn capability(applied: &AlsaAppliedConfig) -> Result<OutputCapability, String> {
    let adjusted = applied.buffer_frames != applied.requested.buffer_frames
        || applied.period_frames != applied.requested.period_frames;
    if applied.period_frames == 0
        || applied.period_frames >= applied.buffer_frames
        || applied.period_frames as usize > AudioLimits::MAX_RENDER_FRAMES
        || applied.format != applied.requested.format
        || adjusted != applied.sizing_adjusted
        || (adjusted && !applied.requested.allow_size_rounding)
    {
        return Err("applied ALSA output metadata is inconsistent".into());
    }
    let cap = OutputCapability {
        host: SettingsHost::Linux,
        current_args: vec![
            "--alsa".into(),
            applied.requested.device.clone(),
            "--buffer-frames".into(),
            applied.buffer_frames.to_string(),
            "--period-frames".into(),
            applied.period_frames.to_string(),
        ],
    };
    cap.validate()?;
    Ok(cap)
}

pub fn capability_for_output(output: &AlsaReplacementOutput) -> Result<OutputCapability, String> {
    let mut cap = capability(output.stream().configuration())?;
    if output.channel_matrix().is_some_and(|matrix| {
        matrix.target_channels() != output.stream().configuration().format.channels()
    }) {
        return Err("applied channel matrix dimensions differ from native output".into());
    }
    cap.current_args.push("--output-matrix".into());
    cap.current_args.push(match output.channel_matrix() {
        Some(matrix) => matrix_text(matrix)?,
        None => "exact".into(),
    });
    cap.validate()?;
    Ok(cap)
}
pub struct NativeAlsaOutputUi {
    bridge: GameplayOutputUi<PlayerOutputUi>,
}
impl NativeAlsaOutputUi {
    pub fn new(
        owner: &NativeAlsaOutputOwner,
        enabled: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut bridge = GameplayOutputUi::new(PlayerOutputUi);
        let cap = if enabled {
            owner.current().map(capability_for_output).transpose()?
        } else {
            None
        };
        bridge.advertise(cap)?;
        Ok(Self { bridge })
    }
    pub fn pending(&self) -> bool {
        self.bridge.pending()
    }
    fn map_request(
        request: &OutputRequest,
        output: &AlsaReplacementOutput,
    ) -> Result<RemixedOutputRequest<AlsaRequest>, String> {
        let applied = output.stream().configuration();
        let mut current = applied.requested.clone();
        current.buffer_frames = applied.buffer_frames;
        current.period_frames = applied.period_frames;
        remixed_request_for_args(&current, output.channel_matrix(), &request.args)
    }

    pub fn service(
        &mut self,
        owner: &mut NativeAlsaOutputOwner,
        context: GameplayOutputContext<'_, PresentationDiscipline>,
        now: ClockPoint,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.bridge.service(
            owner,
            context,
            now,
            &mut Self::map_request,
            &mut capability_for_output,
        )
    }
    pub fn service_audio(
        &mut self,
        owner: &mut NativeAlsaOutputOwner,
        context: GameplayAudioOutputContext<'_>,
        now: ClockPoint,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.bridge.service_audio(
            owner,
            context,
            now,
            &mut Self::map_request,
            &mut capability_for_output,
        )
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel_platform::audio::SampleEncoding;
    fn current() -> AlsaRequest {
        AlsaRequest {
            device: "null".into(),
            format: DeviceFormat::new(48_000, 1, SampleEncoding::Float32, None).unwrap(),
            period_frames: 64,
            buffer_frames: 256,
            allow_size_rounding: false,
            monotonic_domain: beatkernel::time::ClockDomainId(1),
        }
    }
    fn args(flag: &str, value: &str) -> Vec<String> {
        vec![flag.into(), value.into()]
    }
    #[test]
    fn live_matrix_mapping_preserves_rate_and_policy_across_buffer_changes_and_exact_reset() {
        let initial = current();
        let remixed =
            remixed_request_for_args(&initial, None, &args("--output-matrix", "1;0.5")).unwrap();
        assert_eq!(
            (
                remixed.native.format.sample_rate(),
                remixed.native.format.channels()
            ),
            (48_000, 2)
        );
        let preserved = remixed_request_for_args(
            &remixed.native,
            remixed.matrix.as_ref(),
            &args("--period-frames", "32"),
        )
        .unwrap();
        assert_eq!(preserved.matrix, remixed.matrix);
        assert_eq!(preserved.native.period_frames, 32);
        let blank = remixed_request_for_args(
            &preserved.native,
            preserved.matrix.as_ref(),
            &args("--output-matrix", ""),
        )
        .unwrap();
        assert_eq!(blank.matrix, remixed.matrix);
        let reset = remixed_request_for_args(
            &preserved.native,
            preserved.matrix.as_ref(),
            &args("--output-matrix", "exact"),
        )
        .unwrap();
        assert!(reset.matrix.is_none());
        assert_eq!(reset.native.format.channels(), 1);
        assert_eq!(initial.format.channels(), 1);
    }
    #[test]
    fn bad_matrix_rows_refuse_before_any_native_request_or_previous_data_mutation() {
        let initial = current();
        for text in ["1,0", "NaN", "1;", "1\n;0"] {
            assert!(
                remixed_request_for_args(&initial, None, &args("--output-matrix", text)).is_err()
            );
        }
        assert!(request_for_args(&initial, &args("--output-matrix", "1;1")).is_err());
        assert_eq!(initial, current());
    }

    #[test]
    #[ignore = "explicit live settings/native ALSA null diagnostic; no acoustic proof"]
    fn actual_live_matrix_settings_apply_preserve_and_reset_with_original_mixer() {
        use crate::gameplay::output::ports::OutputReplacementBackend;
        use crate::gameplay_output_owner::fixtures::initial;
        use beatkernel::audio::StoppedMixerSource;
        let (mut memory, mut producer, _trace) = initial(vec![]);
        producer.request_pause(true);
        let mut mixer = memory.mixer.take().unwrap();
        mixer.render(&mut [0.; 1]).unwrap();
        let mut native = current();
        native.format = DeviceFormat::new(1000, 1, SampleEncoding::Float32, None).unwrap();
        native.period_frames = 2;
        native.buffer_frames = 8;
        let mut backend = RemixedOutputBackend::new(AlsaReplacementBackend);
        let mut request = RemixedOutputRequest::strict(native);
        for (epoch, expected_channels, expected_text) in
            [(0, 1, "exact"), (1, 2, "1;0.5"), (2, 1, "exact")]
        {
            let mut output = backend
                .open(request, mixer, epoch)
                .unwrap_or_else(|f| panic!("{}", f.error()));
            backend.start(&mut output).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while output.stream().snapshot().submitted_frames < 2 {
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
            let cap = capability_for_output(&output).unwrap();
            let view = NativeSettings::output_only(&cap.current_args, SettingsHost::Linux).unwrap();
            assert_eq!(
                view.fields()
                    .iter()
                    .find(|f| f.flag == "--output-matrix")
                    .unwrap()
                    .value,
                expected_text
            );
            assert_eq!(
                output.stream().configuration().format.channels(),
                expected_channels
            );
            let next = if epoch == 0 { "1;0.5" } else { "exact" };
            request = remixed_request_for_args(
                &output.stream().configuration().requested,
                output.channel_matrix(),
                &args("--output-matrix", next),
            )
            .unwrap();
            if epoch == 1 {
                let preserved = remixed_request_for_args(
                    &output.stream().configuration().requested,
                    output.channel_matrix(),
                    &args("--period-frames", "2"),
                )
                .unwrap();
                assert_eq!(preserved.matrix.as_ref(), output.channel_matrix());
                assert!(remixed_request_for_args(
                    &output.stream().configuration().requested,
                    output.channel_matrix(),
                    &args("--output-matrix", "1,0")
                )
                .is_err());
            }
            backend.retire(&mut output).unwrap();
            mixer = output.take_stopped_mixer().unwrap().unwrap();
            assert_eq!(mixer.config().format().channels(), 1);
            assert!(mixer.is_paused());
        }
    }
}
