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

#[cfg(any(feature = "asio-sdk", test))]
pub(super) fn asio_driver_index<'a>(
    device: &str,
    ids: impl Iterator<Item = &'a str>,
) -> Result<usize, String> {
    let canonical = beatkernel_platform::audio::asio::canonical_asio_clsid(device)
        .map_err(|e| e.to_string())?;
    let mut found = None;
    for (index, id) in ids.enumerate() {
        if id == canonical {
            if found.is_some() {
                return Err("ambiguous ASIO driver registration".into());
            }
            found = Some(index);
        }
    }
    found.ok_or_else(|| "selected ASIO driver is absent from the current registry view".into())
}

#[cfg(any(feature = "asio-sdk", test))]
pub(super) fn asio_request(
    device: &str,
    buffer: beatkernel_platform::audio::asio::AsioBufferRequest,
    channels: &[u32],
    matrix: Option<&ChannelMatrix>,
    args: &[String],
) -> Result<
    (
        String,
        beatkernel_platform::audio::asio::AsioBufferRequest,
        Vec<u32>,
        Option<ChannelMatrix>,
    ),
    String,
> {
    use beatkernel_platform::audio::asio::AsioBufferRequest;
    if channels.is_empty() || channels.len() > 32 {
        return Err("ASIO selected channel count is outside core limits".into());
    }
    let draft = NativeSettings::output_only(args, SettingsHost::Windows)?;
    let mut buffer = buffer;
    let mut device = beatkernel_platform::audio::asio::canonical_asio_clsid(device)
        .map_err(|e| e.to_string())?;
    let mut selected = channels.to_vec();
    let mut text = "";
    for field in draft
        .fields()
        .iter()
        .filter(|field| !field.value.is_empty())
    {
        match field.flag {
            "--device" => {
                device = beatkernel_platform::audio::asio::canonical_asio_clsid(&field.value)
                    .map_err(|e| e.to_string())?
            }
            "--buffer" => {
                buffer = match super::size(&field.value).map_err(|e| e.to_string())? {
                    None => AsioBufferRequest::DriverPreferred,
                    Some((true, count)) if count as usize <= AudioLimits::MAX_RENDER_FRAMES => {
                        AsioBufferRequest::Frames(count as u32)
                    }
                    _ => {
                        return Err(
                            "ASIO buffer requires preferred/default or bounded exact frames".into(),
                        );
                    }
                }
            }
            "--output-matrix" => text = &field.value,
            "--output-channels" => {
                selected = super::parse_output_channels(&field.value).map_err(|e| e.to_string())?
            }
            _ => return Err("ASIO live replacement does not support a period field".into()),
        }
    }
    let source = matrix.map_or(channels.len() as u16, ChannelMatrix::source_channels);
    let (target, matrix) = select_matrix(source, matrix, text)?;
    if target as usize != selected.len() {
        return Err("ASIO matrix target width must match selected driver channels".into());
    }
    Ok((device, buffer, selected, matrix))
}

#[cfg(any(feature = "asio-sdk", test))]
pub(super) fn asio_capability(
    device: &str,
    buffer: beatkernel_platform::audio::asio::AsioBufferRequest,
    channels: &[u32],
    matrix: Option<&ChannelMatrix>,
) -> Result<OutputCapability, String> {
    if channels.is_empty()
        || channels.len() > 32
        || channels.iter().enumerate().any(|(index, channel)| {
            *channel > i32::MAX as u32 || channels[..index].contains(channel)
        })
        || matches!(buffer, beatkernel_platform::audio::asio::AsioBufferRequest::Frames(frames) if frames == 0 || frames as usize > AudioLimits::MAX_RENDER_FRAMES)
        || matrix.is_some_and(|matrix| matrix.target_channels() as usize != channels.len())
    {
        return Err("ASIO applied channel matrix differs from selected channels".into());
    }
    let cap = OutputCapability {
        host: SettingsHost::Windows,
        current_args: vec![
            "--device".into(),
            beatkernel_platform::audio::asio::canonical_asio_clsid(device)
                .map_err(|e| e.to_string())?,
            "--buffer".into(),
            match buffer {
                beatkernel_platform::audio::asio::AsioBufferRequest::DriverPreferred => {
                    "default".into()
                }
                beatkernel_platform::audio::asio::AsioBufferRequest::Frames(frames) => {
                    format!("frames:{frames}")
                }
            },
            "--output-matrix".into(),
            match matrix {
                Some(matrix) => matrix_text(matrix)?,
                None => "exact".into(),
            },
        ],
    };
    let mut cap = cap;
    cap.current_args.extend([
        "--output-channels".into(),
        channels
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(","),
    ]);
    cap.validate()?;
    Ok(cap)
}

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
    let mut shared = match current.mode() {
        beatkernel_platform::audio::AudioStreamMode::Shared(policy) => policy,
        _ => beatkernel_platform::audio::SharedPeriodPolicy::EnginePeriod,
    };
    let mut exclusive = current.mode() == beatkernel_platform::audio::AudioStreamMode::Exclusive;
    let mut text = "";
    for field in draft
        .fields()
        .iter()
        .filter(|field| !field.value.is_empty())
    {
        match field.flag {
            "--device" => device = AudioDeviceId(field.value.clone()),
            "--mode" => {
                exclusive = match field.value.as_str() {
                    "exclusive" => true,
                    "shared" => false,
                    _ => return Err("WASAPI mode must be shared or exclusive".into()),
                }
            }
            "--shared-policy" => {
                shared = match field.value.as_str() {
                    "engine" => beatkernel_platform::audio::SharedPeriodPolicy::EnginePeriod,
                    "legacy" => beatkernel_platform::audio::SharedPeriodPolicy::DeviceDefault,
                    _ => return Err("WASAPI shared policy must be engine or legacy".into()),
                }
            }
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
    let mode = if exclusive {
        beatkernel_platform::audio::AudioStreamMode::Exclusive
    } else {
        beatkernel_platform::audio::AudioStreamMode::Shared(shared)
    };
    if mode
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
    let native = AudioStreamRequest::new(device, current.backend(), mode, format, buffer, period)
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
            "--mode".into(),
            if applied.requested.mode() == beatkernel_platform::audio::AudioStreamMode::Exclusive {
                "exclusive".into()
            } else {
                "shared".into()
            },
            "--shared-policy".into(),
            match applied.requested.mode() {
                beatkernel_platform::audio::AudioStreamMode::Shared(
                    beatkernel_platform::audio::SharedPeriodPolicy::DeviceDefault,
                ) => "legacy".into(),
                _ => "engine".into(),
            },
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
    const DRIVER: &str = "{ABCDEF12-3456-7890-ABCD-EF1234567890}";
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
            vec!["--mode", "invalid"],
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

    #[test]
    fn asio_draft_preserves_trusted_driver_channels_and_matrix_when_buffer_changes() {
        use beatkernel_platform::audio::asio::AsioBufferRequest;
        let matrix = ChannelMatrix::new(2, 2, &[0., 1., 1., 0.]).unwrap();
        let (_, buffer, channels, retained) = asio_request(
            DRIVER,
            AsioBufferRequest::DriverPreferred,
            &[0, 1],
            Some(&matrix),
            &["--buffer".into(), "frames:128".into()],
        )
        .unwrap();
        assert_eq!(buffer, AsioBufferRequest::Frames(128));
        assert_eq!(retained.as_ref(), Some(&matrix));
        let cap = asio_capability(DRIVER, buffer, &channels, retained.as_ref()).unwrap();
        let fields = cap.settings().unwrap();
        assert_eq!(
            fields
                .fields()
                .iter()
                .map(|field| field.flag)
                .collect::<Vec<_>>(),
            vec![
                "--device",
                "--buffer",
                "--output-channels",
                "--output-matrix"
            ]
        );
        assert_eq!(
            asio_request(
                DRIVER,
                buffer,
                &[0, 1],
                retained.as_ref(),
                &cap.current_args
            )
            .unwrap(),
            (DRIVER.to_owned(), buffer, channels, retained)
        );
        let (_, preferred, _, cleared) = asio_request(
            DRIVER,
            buffer,
            &[0, 1],
            Some(&matrix),
            &[
                "--buffer".into(),
                "default".into(),
                "--output-matrix".into(),
                "exact".into(),
            ],
        )
        .unwrap();
        assert_eq!(preferred, AsioBufferRequest::DriverPreferred);
        assert!(cleared.is_none());
    }

    #[test]
    fn asio_draft_refuses_unsupported_driver_period_duration_and_target_width() {
        use beatkernel_platform::audio::asio::AsioBufferRequest;
        for args in [
            vec!["--device", "different-driver"],
            vec!["--period", "frames:64"],
            vec!["--buffer", "ns:1000000"],
            vec!["--buffer", "frames:0"],
            vec!["--output-matrix", "1,0"],
            vec!["--output-matrix", "1,0;0,1;1,1"],
        ] {
            assert!(
                asio_request(
                    DRIVER,
                    AsioBufferRequest::DriverPreferred,
                    &[0, 1],
                    None,
                    &args.into_iter().map(str::to_owned).collect::<Vec<_>>()
                )
                .is_err()
            );
        }
        assert!(asio_capability(DRIVER, AsioBufferRequest::Frames(0), &[0, 1], None).is_err());
        assert!(asio_capability(DRIVER, AsioBufferRequest::DriverPreferred, &[], None).is_err());
        assert!(
            asio_capability(DRIVER, AsioBufferRequest::DriverPreferred, &[0, 0], None).is_err()
        );
        assert!(
            asio_capability(
                DRIVER,
                AsioBufferRequest::DriverPreferred,
                &[u32::MAX],
                None
            )
            .is_err()
        );
    }

    #[test]
    fn asio_channel_reorder_and_resize_keep_original_source_grid_and_explicit_reset() {
        use beatkernel_platform::audio::asio::AsioBufferRequest;
        let original = [0, 1];
        let (_, buffer, reordered, matrix) = asio_request(
            DRIVER,
            AsioBufferRequest::Frames(64),
            &original,
            None,
            &["--output-channels".into(), "7,4".into()],
        )
        .unwrap();
        assert_eq!(reordered, [7, 4]);
        assert!(matrix.is_none());
        assert!(
            asio_request(
                DRIVER,
                buffer,
                &reordered,
                None,
                &["--output-channels".into(), "7,4,2".into()]
            )
            .is_err()
        );
        let (_, _, resized, matrix) = asio_request(
            DRIVER,
            buffer,
            &reordered,
            None,
            &[
                "--output-channels".into(),
                "7,4,2".into(),
                "--output-matrix".into(),
                "1,0;0,1;0.5,0.5".into(),
            ],
        )
        .unwrap();
        assert_eq!(resized, [7, 4, 2]);
        assert_eq!(matrix.as_ref().unwrap().source_channels(), 2);
        let (_, _, same, retained) = asio_request(
            DRIVER,
            buffer,
            &resized,
            matrix.as_ref(),
            &["--buffer".into(), "frames:128".into()],
        )
        .unwrap();
        assert_eq!(same, resized);
        assert_eq!(retained, matrix);
        assert!(
            asio_request(
                DRIVER,
                buffer,
                &resized,
                matrix.as_ref(),
                &["--output-matrix".into(), "exact".into()]
            )
            .is_err()
        );
        let (_, _, reset, cleared) = asio_request(
            DRIVER,
            buffer,
            &resized,
            matrix.as_ref(),
            &[
                "--output-channels".into(),
                "4,7".into(),
                "--output-matrix".into(),
                "exact".into(),
            ],
        )
        .unwrap();
        assert_eq!(reset, [4, 7]);
        assert!(cleared.is_none());
    }

    #[test]
    fn asio_channel_syntax_is_shared_with_launch_and_never_reaches_wasapi() {
        use beatkernel_platform::audio::asio::AsioBufferRequest;
        for text in ["", "0,0", "0,", "-1", "+1", "2147483648", "4294967296"] {
            assert!(super::super::parse_output_channels(text).is_err());
            if !text.is_empty() {
                assert!(
                    asio_request(
                        DRIVER,
                        AsioBufferRequest::DriverPreferred,
                        &[0, 1],
                        None,
                        &["--output-channels".into(), text.into()]
                    )
                    .is_err()
                );
            }
        }
        let oversized = (0..33).map(|n| n.to_string()).collect::<Vec<_>>().join(",");
        assert!(super::super::parse_output_channels(&oversized).is_err());
        assert!(request(current(), None, &["--output-channels".into(), "0,1".into()]).is_err());
    }

    #[test]
    fn wasapi_live_mode_and_policy_keep_source_and_validate_final_mode_period() {
        use beatkernel_platform::audio::{AudioStreamMode, SharedPeriodPolicy};
        let original = current();
        let engine = request(
            original.clone(),
            None,
            &[
                "--mode".into(),
                "shared".into(),
                "--shared-policy".into(),
                "engine".into(),
                "--period".into(),
                "frames:32".into(),
            ],
        )
        .unwrap();
        assert_eq!(
            engine.native.mode(),
            AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod)
        );
        assert_eq!(engine.native.format(), original.format());
        assert_eq!(engine.native.negotiation(), original.negotiation());
        assert!(
            request(
                engine.native.clone(),
                None,
                &["--shared-policy".into(), "legacy".into()]
            )
            .is_err()
        );
        let legacy = request(
            engine.native,
            None,
            &[
                "--shared-policy".into(),
                "legacy".into(),
                "--period".into(),
                "default".into(),
            ],
        )
        .unwrap();
        assert_eq!(
            legacy.native.mode(),
            AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault)
        );
        let exclusive = request(
            legacy.native,
            None,
            &[
                "--mode".into(),
                "exclusive".into(),
                "--period".into(),
                "frames:64".into(),
            ],
        )
        .unwrap();
        assert_eq!(exclusive.native.mode(), AudioStreamMode::Exclusive);
        assert_eq!(exclusive.native.period(), PeriodRequest::Frames(64));
        for args in [
            vec!["--mode", "invalid"],
            vec!["--shared-policy", "invalid"],
        ] {
            assert!(
                request(
                    original.clone(),
                    None,
                    &args.into_iter().map(str::to_owned).collect::<Vec<_>>()
                )
                .is_err()
            );
        }
    }

    #[test]
    fn driver_switch_is_canonical_and_requires_exact_one_installed_registration() {
        use beatkernel_platform::audio::asio::AsioBufferRequest;
        let other = "11111111-2222-3333-4444-555555555555";
        let canonical = "{11111111-2222-3333-4444-555555555555}";
        let (selected, buffer, channels, matrix) = asio_request(
            DRIVER,
            AsioBufferRequest::Frames(64),
            &[7, 4],
            None,
            &["--device".into(), other.into()],
        )
        .unwrap();
        assert_eq!(selected, canonical);
        assert_eq!(buffer, AsioBufferRequest::Frames(64));
        assert_eq!(channels, [7, 4]);
        assert!(matrix.is_none());
        assert_eq!(
            asio_driver_index(other, [DRIVER, canonical].into_iter()).unwrap(),
            1
        );
        assert!(asio_driver_index(other, [DRIVER].into_iter()).is_err());
        assert!(asio_driver_index(other, [canonical, canonical].into_iter()).is_err());
        let cap = asio_capability(&selected, buffer, &channels, None).unwrap();
        let settings = cap.settings().unwrap();
        let field = settings
            .fields()
            .iter()
            .find(|field| field.flag == "--device")
            .unwrap();
        assert_eq!(field.label, "TRUSTED ASIO DRIVER CLSID");
        assert!(field.hint.contains("APPLY loads"));
        assert_eq!(
            asio_request(&selected, buffer, &channels, None, &cap.current_args)
                .unwrap()
                .0,
            canonical
        );
    }
}
