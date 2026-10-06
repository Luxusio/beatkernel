//! Portable WASAPI draft mapping; no endpoint is opened before validation.
use beatkernel::audio::{AudioLimits, ChannelMatrix};
use beatkernel_bms_runtime::{
    gameplay::output::domain::{
        control::OutputCapability,
        remix::{RemixedOutputRequest, select_matrix, matrix_text},
    },
    settings::{NativeSettings, SettingsHost},
};
use beatkernel_platform::audio::{
    AudioBackendKind, AudioDeviceId, AudioStreamRequest, AppliedStreamConfig, BufferRequest,
    PeriodRequest, DeviceFormat,
};

pub(super) fn request(
    current: AudioStreamRequest,
    matrix: Option<&ChannelMatrix>,
    args: &[String],
) -> Result<RemixedOutputRequest<AudioStreamRequest>, String> {
    if current.backend() != AudioBackendKind::Wasapi {
        return Err("live draft requires WASAPI output".into());
    }
    let draft = NativeSettings::output_only(args, SettingsHost::Windows)?;
    let mut device = current.device().clone();
    let mut buffer = current.buffer();
    let mut period = current.period();
    let mut text = "";
    for field in draft
        .fields()
        .iter()
        .filter(|field| !field.value.is_empty())
    {
        match field.flag {
            "--device" => device = AudioDeviceId(field.value.clone()),
            "--buffer" => {
                buffer = match super::size(&field.value).map_err(|e| e.to_string())? {
                    None => BufferRequest::DeviceDefault,
                    Some((true, n)) => BufferRequest::Frames(n as u32),
                    Some((false, n)) => {
                        BufferRequest::Duration(beatkernel::time::Duration::from_nanos(n as i64))
                    }
                }
            }
            "--period" => {
                period = match super::size(&field.value).map_err(|e| e.to_string())? {
                    None => PeriodRequest::DeviceDefault,
                    Some((true, n)) => PeriodRequest::Frames(n as u32),
                    Some((false, n)) => {
                        PeriodRequest::Duration(beatkernel::time::Duration::from_nanos(n as i64))
                    }
                }
            }
            "--output-matrix" => text = &field.value,
            _ => return Err("unsupported WASAPI live output field".into()),
        }
    }
    for frames in [
        match buffer {
            BufferRequest::Frames(n) => Some(n),
            _ => None,
        },
        match period {
            PeriodRequest::Frames(n) => Some(n),
            _ => None,
        },
    ]
    .into_iter()
    .flatten()
    {
        if frames as usize > AudioLimits::MAX_RENDER_FRAMES {
            return Err("WASAPI size exceeds render capacity".into());
        }
    }
    if current.mode()
        == beatkernel_platform::audio::AudioStreamMode::Shared(
            beatkernel_platform::audio::SharedPeriodPolicy::DeviceDefault,
        )
        && period != PeriodRequest::DeviceDefault
    {
        return Err("legacy shared WASAPI requires the default period".into());
    }
    let source = matrix.map_or(current.format().channels(), ChannelMatrix::source_channels);
    let (channels, matrix) = select_matrix(source, matrix, text)?;
    let format = DeviceFormat::new(
        current.format().sample_rate(),
        channels,
        current.format().encoding(),
        if channels == current.format().channels() {
            current.format().channel_mask()
        } else {
            None
        },
    )
    .map_err(|e| e.to_string())?;
    let native = AudioStreamRequest::new(
        device,
        current.backend(),
        current.mode(),
        format,
        buffer,
        period,
    )
    .map_err(|e| e.to_string())?
    .with_negotiation(current.negotiation());
    Ok(RemixedOutputRequest { native, matrix })
}
fn buffer_text(size: BufferRequest) -> String {
    match size {
        BufferRequest::DeviceDefault => "default".into(),
        BufferRequest::Frames(n) => format!("frames:{n}"),
        BufferRequest::Duration(n) => format!("ns:{}", n.as_nanos()),
    }
}
fn period_text(size: PeriodRequest) -> String {
    match size {
        PeriodRequest::DeviceDefault => "default".into(),
        PeriodRequest::Frames(n) => format!("frames:{n}"),
        PeriodRequest::Duration(n) => format!("ns:{}", n.as_nanos()),
    }
}
pub(super) fn capability(
    applied: &AppliedStreamConfig,
    matrix: Option<&ChannelMatrix>,
) -> Result<OutputCapability, String> {
    if applied.requested.backend() != AudioBackendKind::Wasapi
        || applied.format != applied.requested.format()
        || applied.buffer_frames == 0
        || applied.buffer_frames as usize > AudioLimits::MAX_RENDER_FRAMES
        || matrix.is_some_and(|matrix| matrix.target_channels() != applied.format.channels())
    {
        return Err("inconsistent applied WASAPI output metadata".into());
    }
    let cap = OutputCapability {
        host: SettingsHost::Windows,
        current_args: vec![
            "--device".into(),
            applied.requested.device().0.clone(),
            "--buffer".into(),
            buffer_text(applied.requested.buffer()),
            "--period".into(),
            period_text(applied.requested.period()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use beatkernel_platform::audio::{AudioStreamMode, SampleEncoding, NegotiationPolicy};
    fn current() -> AudioStreamRequest {
        AudioStreamRequest::new(
            AudioDeviceId("endpoint-A".into()),
            AudioBackendKind::Wasapi,
            AudioStreamMode::Exclusive,
            DeviceFormat::new(48_000, 1, SampleEncoding::Float32, None).unwrap(),
            BufferRequest::Frames(64),
            PeriodRequest::DeviceDefault,
        )
        .unwrap()
        .with_negotiation(NegotiationPolicy::AllowSupportedRounding)
    }
    #[test]
    fn live_wasapi_draft_preserves_mode_rate_policy_and_original_matrix_source() {
        let original = current();
        let request = request(
            original.clone(),
            None,
            &["--output-matrix".into(), "1;0.5".into()],
        )
        .unwrap();
        assert_eq!(request.native.format().channels(), 2);
        assert_eq!(request.native.mode(), original.mode());
        assert_eq!(request.native.negotiation(), original.negotiation());
        let updated = super::request(
            request.native.clone(),
            request.matrix.as_ref(),
            &["--buffer".into(), "frames:128".into()],
        )
        .unwrap();
        assert_eq!(updated.matrix, request.matrix);
        assert_eq!(updated.native.format().sample_rate(), 48_000);
        let exact = super::request(
            updated.native,
            updated.matrix.as_ref(),
            &["--output-matrix".into(), "exact".into()],
        )
        .unwrap();
        assert_eq!(exact.native.format().channels(), 1);
        assert!(exact.matrix.is_none());
    }
    #[test]
    fn invalid_wasapi_drafts_fail_before_any_native_owner_is_required() {
        for args in [
            vec!["--buffer", "frames:0"],
            vec!["--buffer", "frames:4294967296"],
            vec!["--period", "ns:9223372036854775808"],
            vec!["--output-matrix", "1,0"],
            vec!["--device", "bad\0endpoint"],
            vec!["--mode", "shared"],
        ] {
            assert!(
                request(
                    current(),
                    None,
                    &args.into_iter().map(str::to_owned).collect::<Vec<_>>()
                )
                .is_err()
            );
        }
        let original = current();
        let unchanged = request(original.clone(), None, &[]).unwrap();
        assert_eq!(unchanged.native, original);
        let changed = request(
            original,
            None,
            &[
                "--period".into(),
                "ns:1000000".into(),
                "--device".into(),
                "endpoint-B".into(),
            ],
        )
        .unwrap();
        assert_eq!(changed.native.device().0, "endpoint-B");
        assert_eq!(
            changed.native.period(),
            PeriodRequest::Duration(beatkernel::time::Duration::from_nanos(1_000_000))
        );
    }

    #[test]
    fn advertised_wasapi_draft_round_trips_requested_policy_and_canonical_matrix() {
        let mapped = request(current(), None, &["--output-matrix".into(), "1;0.5".into()]).unwrap();
        let mut applied = AppliedStreamConfig {
            format: mapped.native.format(),
            requested: mapped.native.clone(),
            buffer_frames: 64,
            buffer_duration: beatkernel::time::Duration::from_nanos(1_333_334),
            period_frames: Some(64),
            period_duration: beatkernel::time::Duration::from_nanos(1_333_334),
            stream_latency: beatkernel::time::Duration::ZERO,
            sizing_adjusted: false,
        };
        let cap = capability(&applied, mapped.matrix.as_ref()).unwrap();
        let reply = request(
            mapped.native.clone(),
            mapped.matrix.as_ref(),
            &cap.current_args,
        )
        .unwrap();
        assert_eq!(reply.native, mapped.native);
        assert_eq!(reply.matrix, mapped.matrix);
        applied.buffer_frames = 0;
        assert!(capability(&applied, mapped.matrix.as_ref()).is_err());
        applied.buffer_frames = 64;
        applied.format = current().format();
        assert!(capability(&applied, mapped.matrix.as_ref()).is_err());
    }

    #[test]
    fn legacy_shared_period_refusal_preserves_the_current_request_policy() {
        let original = current();
        let shared = AudioStreamRequest::new(
            original.device().clone(),
            original.backend(),
            AudioStreamMode::Shared(beatkernel_platform::audio::SharedPeriodPolicy::DeviceDefault),
            original.format(),
            original.buffer(),
            PeriodRequest::DeviceDefault,
        )
        .unwrap();
        assert!(
            request(
                shared.clone(),
                None,
                &["--period".into(), "frames:32".into()]
            )
            .is_err()
        );
        let preserved = request(
            shared.clone(),
            None,
            &["--buffer".into(), "frames:128".into()],
        )
        .unwrap();
        assert_eq!(preserved.native.mode(), shared.mode());
        assert_eq!(preserved.native.period(), PeriodRequest::DeviceDefault);
    }
}
