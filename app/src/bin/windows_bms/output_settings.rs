//! Portable target-first Windows output draft mapping; no endpoint is opened before validation.
use beatkernel::audio::{AudioFormat, AudioLimits, ChannelMatrix};
use beatkernel_bms_runtime::{
    gameplay::output::domain::{
        control::OutputCapability,
        remix::{matrix_text, select_matrix, RemixedOutputRequest},
    },
    settings::{NativeSettings, SettingsHost},
};
use beatkernel_platform::audio::{
    AppliedStreamConfig, AudioBackendKind, AudioDeviceId, AudioStreamRequest, BufferRequest,
    DeviceFormat, PeriodRequest,
};

pub(super) fn switch_capability(
    mut cap: OutputCapability,
    backend: super::Backend,
    view: Option<super::AsioView>,
) -> Result<OutputCapability, String> {
    cap.current_args.extend([
        "--backend".into(),
        match backend {
            super::Backend::Wasapi => "wasapi",
            super::Backend::Asio => "asio",
        }
        .into(),
    ]);
    for flag in [
        "--mode",
        "--shared-policy",
        "--period",
        "--output-channels",
        "--asio-view",
        "--asio-system-clock",
        "--asio-timer-error-ns",
        "--asio-drift-error-ns",
        "--asio-latency-error-ns",
        "--asio-anchor-age-ns",
    ] {
        if !cap.current_args.chunks_exact(2).any(|pair| pair[0] == flag) {
            let value = match flag {
                "--asio-view" if backend == super::Backend::Asio => {
                    match view.ok_or("ASIO current registry view unavailable")? {
                        super::AsioView::Native => "native",
                        super::AsioView::Bits32 => "32",
                        super::AsioView::Bits64 => "64",
                    }
                }
                "--asio-system-clock" if backend == super::Backend::Asio => "multimedia",
                _ => "",
            };
            cap.current_args.extend([flag.into(), value.into()]);
        }
    }
    cap.validate()?;
    Ok(cap)
}

pub(super) struct WasapiTargetPlan {
    pub(super) device: Option<AudioDeviceId>,
    pub(super) mode: beatkernel_platform::audio::AudioStreamMode,
    pub(super) format: DeviceFormat,
    pub(super) buffer: BufferRequest,
    pub(super) period: PeriodRequest,
    pub(super) negotiation: beatkernel_platform::audio::NegotiationPolicy,
    pub(super) matrix: Option<ChannelMatrix>,
}
impl WasapiTargetPlan {
    pub(super) fn into_request(
        self,
        default_device: Option<AudioDeviceId>,
    ) -> Result<RemixedOutputRequest<AudioStreamRequest>, String> {
        let device = self
            .device
            .or(default_device)
            .ok_or("OS default WASAPI endpoint must be resolved before output replacement")?;
        let native = AudioStreamRequest::new(
            device,
            AudioBackendKind::Wasapi,
            self.mode,
            self.format,
            self.buffer,
            self.period,
        )
        .map_err(|e| e.to_string())?
        .with_negotiation(self.negotiation);
        Ok(RemixedOutputRequest {
            native,
            matrix: self.matrix,
        })
    }
}
#[cfg_attr(not(feature = "asio-sdk"), allow(dead_code))]
pub(super) struct AsioTargetPlan {
    pub(super) device: String,
    pub(super) view: super::AsioView,
    pub(super) channels: Vec<u32>,
    pub(super) buffer: beatkernel_platform::audio::asio::AsioBufferRequest,
    pub(super) bounds: AsioClockBounds,
    pub(super) sample_rate: u32,
    pub(super) matrix: Option<ChannelMatrix>,
}
pub(super) enum TargetPlan {
    Wasapi(WasapiTargetPlan),
    Asio(AsioTargetPlan),
}

/// Select target first; inactive backend fields are never interpreted.
pub(super) fn plan_target(
    current: &OutputCapability,
    current_wasapi: Option<&AudioStreamRequest>,
    source: AudioFormat,
    matrix: Option<&ChannelMatrix>,
    args: &[String],
    asio_available: bool,
) -> Result<TargetPlan, String> {
    let current_draft = NativeSettings::output_only(&current.current_args, SettingsHost::Windows)?;
    let draft = NativeSettings::output_only(args, SettingsHost::Windows)?;
    let value = |draft: &NativeSettings, flag: &str| {
        draft
            .fields()
            .iter()
            .find(|f| f.flag == flag && !f.value.is_empty())
            .map(|f| f.value.clone())
    };
    let current_backend = match value(&current_draft, "--backend").as_deref() {
        Some("wasapi") => super::Backend::Wasapi,
        Some("asio") => super::Backend::Asio,
        None if value(&current_draft, "--output-channels").is_some() => super::Backend::Asio,
        None => super::Backend::Wasapi,
        _ => return Err("backend must be wasapi or asio".into()),
    };
    let target = match value(&draft, "--backend").as_deref() {
        Some("wasapi") => super::Backend::Wasapi,
        Some("asio") => super::Backend::Asio,
        None => current_backend,
        _ => return Err("backend must be wasapi or asio".into()),
    };
    let filtered: Vec<String> = draft
        .fields()
        .iter()
        .filter(|f| args.chunks_exact(2).any(|pair| pair[0] == f.flag))
        .filter(|f| match target {
            super::Backend::Wasapi => matches!(
                f.flag,
                "--device"
                    | "--buffer"
                    | "--output-matrix"
                    | "--mode"
                    | "--period"
                    | "--shared-policy"
            ),
            super::Backend::Asio => matches!(
                f.flag,
                "--device"
                    | "--buffer"
                    | "--output-matrix"
                    | "--output-channels"
                    | "--asio-timer-error-ns"
                    | "--asio-drift-error-ns"
                    | "--asio-latency-error-ns"
                    | "--asio-anchor-age-ns"
            ),
        })
        .flat_map(|f| [f.flag.to_owned(), f.value.clone()])
        .collect();
    if target == super::Backend::Wasapi {
        return wasapi_plan(
            if current_backend == target {
                current_wasapi
            } else {
                None
            },
            source,
            matrix,
            &filtered,
        )
        .map(TargetPlan::Wasapi);
    }
    if !asio_available {
        return Err("ASIO requires build feature asio-sdk; current output retained".into());
    }
    let inherited = |flag: &str| {
        value(&draft, flag).or_else(|| {
            (current_backend == target)
                .then(|| value(&current_draft, flag))
                .flatten()
        })
    };
    let required = |flag: &str| {
        inherited(flag).ok_or_else(|| format!("first ASIO selection requires explicit {flag}"))
    };
    let device = required("--device")?;
    let view = match required("--asio-view")?.as_str() {
        "native" => super::AsioView::Native,
        "32" => super::AsioView::Bits32,
        "64" => super::AsioView::Bits64,
        _ => return Err("ASIO view must be native, 32 or 64".into()),
    };
    if required("--asio-system-clock")? != "multimedia" {
        return Err("ASIO system clock must be explicitly multimedia".into());
    }
    let channels =
        super::parse_output_channels(&required("--output-channels")?).map_err(|e| e.to_string())?;
    let mut bounds_args = Vec::new();
    for flag in [
        "--asio-timer-error-ns",
        "--asio-drift-error-ns",
        "--asio-latency-error-ns",
        "--asio-anchor-age-ns",
    ] {
        bounds_args.extend([flag.into(), required(flag)?]);
    }
    let bounds = asio_clock_request(
        AsioClockBounds {
            timer: 0,
            drift: 0,
            latency: 0,
            age: 0,
        },
        &bounds_args,
    )?;
    let buffer_args = inherited("--buffer")
        .map(|v| vec!["--buffer".into(), v])
        .unwrap_or_default();
    let initial_buffer = match super::size(
        value(&current_draft, "--buffer")
            .as_deref()
            .filter(|_| current_backend == target)
            .unwrap_or("default"),
    )
    .map_err(|e| e.to_string())?
    {
        None => beatkernel_platform::audio::asio::AsioBufferRequest::DriverPreferred,
        Some((true, n)) => beatkernel_platform::audio::asio::AsioBufferRequest::Frames(n as u32),
        _ => return Err("ASIO buffer requires preferred/default or bounded exact frames".into()),
    };
    let mut active = filtered;
    // Target shared buffer value overrides current configuration.
    if !active.chunks_exact(2).any(|p| p[0] == "--buffer") {
        active.extend(buffer_args);
    }
    let (device, buffer, channels, matrix) = asio_request_for_source(
        &device,
        initial_buffer,
        &channels,
        source.channels(),
        matrix,
        &active,
    )?;
    Ok(TargetPlan::Asio(AsioTargetPlan {
        device,
        view,
        channels,
        buffer,
        bounds,
        sample_rate: source.sample_rate(),
        matrix,
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AsioClockBounds {
    pub(super) timer: u64,
    pub(super) drift: u64,
    pub(super) latency: u64,
    pub(super) age: u64,
}
pub(super) fn asio_clock_request(
    current: AsioClockBounds,
    args: &[String],
) -> Result<AsioClockBounds, String> {
    let draft = NativeSettings::output_only(args, SettingsHost::Windows)?;
    let mut bounds = current;
    for field in draft
        .fields()
        .iter()
        .filter(|field| !field.value.is_empty())
    {
        let slot = match field.flag {
            "--asio-timer-error-ns" => &mut bounds.timer,
            "--asio-drift-error-ns" => &mut bounds.drift,
            "--asio-latency-error-ns" => &mut bounds.latency,
            "--asio-anchor-age-ns" => &mut bounds.age,
            _ => continue,
        };
        if !field.value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("ASIO clock bounds require unsigned ASCII decimal nanoseconds".into());
        }
        *slot = field.value.parse::<u64>().map_err(|e| e.to_string())?;
        if *slot > i64::MAX as u64 {
            return Err("ASIO clock bound exceeds signed timestamp range".into());
        }
    }
    beatkernel_platform::audio::asio::MultimediaClockAnchor::validate_bounds(
        bounds.age,
        bounds.timer,
        bounds.drift,
        0,
    )
    .map_err(|e| e.to_string())?;
    Ok(bounds)
}
#[cfg(any(feature = "asio-sdk", test))]
pub(super) fn asio_clock_capability(
    mut cap: OutputCapability,
    bounds: AsioClockBounds,
) -> Result<OutputCapability, String> {
    for (flag, value) in [
        ("--asio-timer-error-ns", bounds.timer),
        ("--asio-drift-error-ns", bounds.drift),
        ("--asio-latency-error-ns", bounds.latency),
        ("--asio-anchor-age-ns", bounds.age),
    ] {
        cap.current_args.extend([flag.into(), value.to_string()]);
    }
    asio_clock_request(bounds, &cap.current_args)?;
    cap.validate()?;
    Ok(cap)
}

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

#[cfg(test)]
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
    asio_request_for_source(
        device,
        buffer,
        channels,
        matrix.map_or(channels.len() as u16, ChannelMatrix::source_channels),
        matrix,
        args,
    )
}
fn asio_request_for_source(
    device: &str,
    buffer: beatkernel_platform::audio::asio::AsioBufferRequest,
    channels: &[u32],
    source: u16,
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
            "--asio-timer-error-ns"
            | "--asio-drift-error-ns"
            | "--asio-latency-error-ns"
            | "--asio-anchor-age-ns" => {}
            _ => return Err("ASIO live replacement does not support a period field".into()),
        }
    }
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

#[cfg(test)]
pub(super) fn request(
    current: AudioStreamRequest,
    matrix: Option<&ChannelMatrix>,
    args: &[String],
) -> Result<RemixedOutputRequest<AudioStreamRequest>, String> {
    if current.backend() != AudioBackendKind::Wasapi {
        return Err("live draft requires WASAPI output".into());
    }
    let source = AudioFormat::new(
        current.format().sample_rate(),
        matrix.map_or(current.format().channels(), ChannelMatrix::source_channels),
    )
    .map_err(|e| e.to_string())?;
    wasapi_plan(Some(&current), source, matrix, args)?.into_request(None)
}
fn wasapi_plan(
    current: Option<&AudioStreamRequest>,
    source: AudioFormat,
    matrix: Option<&ChannelMatrix>,
    args: &[String],
) -> Result<WasapiTargetPlan, String> {
    let draft = NativeSettings::output_only(args, SettingsHost::Windows)?;
    let mut device = current.map(|c| c.device().clone());
    let mut buffer = current.map_or(BufferRequest::DeviceDefault, AudioStreamRequest::buffer);
    let mut period = current.map_or(PeriodRequest::DeviceDefault, AudioStreamRequest::period);
    let mut shared = current.map_or(
        beatkernel_platform::audio::SharedPeriodPolicy::EnginePeriod,
        |c| match c.mode() {
            beatkernel_platform::audio::AudioStreamMode::Shared(policy) => policy,
            _ => beatkernel_platform::audio::SharedPeriodPolicy::EnginePeriod,
        },
    );
    let mut exclusive =
        current.is_some_and(|c| c.mode() == beatkernel_platform::audio::AudioStreamMode::Exclusive);
    // Blank device is an explicit OS-default choice, not an inherited CLSID.
    for field in draft
        .fields()
        .iter()
        .filter(|f| f.flag == "--device" && args.chunks_exact(2).any(|p| p[0] == "--device"))
    {
        if !field.value.is_empty() || current.is_none() {
            device = (!field.value.is_empty()).then(|| AudioDeviceId(field.value.clone()));
        }
    }
    let mut text = "";
    for field in draft
        .fields()
        .iter()
        .filter(|field| !field.value.is_empty())
    {
        match field.flag {
            "--device" => {}
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
    if device.as_ref().is_some_and(|d| d.0.contains('\0')) {
        return Err("invalid WASAPI endpoint".into());
    }
    if device
        .as_ref()
        .is_some_and(|d| beatkernel_platform::audio::asio::canonical_asio_clsid(&d.0).is_ok())
    {
        return Err("ASIO CLSID is not a WASAPI endpoint; choose a WASAPI endpoint or clear the device field for OS default".into());
    }
    let (channels, matrix) = select_matrix(source.channels(), matrix, text)?;
    let format = DeviceFormat::new(
        source.sample_rate(),
        channels,
        current.map_or(beatkernel_platform::audio::SampleEncoding::Float32, |c| {
            c.format().encoding()
        }),
        current
            .filter(|c| channels == c.format().channels())
            .and_then(|c| c.format().channel_mask()),
    )
    .map_err(|e| e.to_string())?;
    Ok(WasapiTargetPlan {
        device,
        mode,
        format,
        buffer,
        period,
        matrix,
        negotiation: current.map_or(
            beatkernel_platform::audio::NegotiationPolicy::Exact,
            AudioStreamRequest::negotiation,
        ),
    })
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
    use beatkernel_platform::audio::{AudioStreamMode, NegotiationPolicy, SampleEncoding};
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
    fn actual_wasapi_switch_capability_emits_inactive_rows_without_changing_backend_label() {
        let requested = current();
        let applied = AppliedStreamConfig {
            format: requested.format(),
            requested: requested.clone(),
            buffer_frames: 64,
            buffer_duration: beatkernel::time::Duration::from_nanos(1_333_333),
            period_frames: None,
            period_duration: beatkernel::time::Duration::ZERO,
            stream_latency: beatkernel::time::Duration::ZERO,
            sizing_adjusted: false,
        };
        let cap = switch_capability(
            capability(&applied, None).unwrap(),
            super::super::Backend::Wasapi,
            None,
        )
        .unwrap();
        let fields = cap.settings().unwrap();
        for flag in [
            "--backend",
            "--asio-view",
            "--asio-system-clock",
            "--output-channels",
            "--asio-anchor-age-ns",
        ] {
            assert!(fields.fields().iter().any(|f| f.flag == flag));
        }
        let device = fields
            .fields()
            .iter()
            .find(|f| f.flag == "--device")
            .unwrap();
        assert_ne!(device.label, "TRUSTED ASIO DRIVER CLSID");
        assert!(fields
            .fields()
            .iter()
            .find(|f| f.flag == "--output-channels")
            .unwrap()
            .value
            .is_empty());
        let TargetPlan::Wasapi(plan) = plan_target(
            &cap,
            Some(&requested),
            requested.format().pcm(),
            None,
            &cap.current_args,
            false,
        )
        .unwrap() else {
            panic!("WASAPI capability changed target")
        };
        assert_eq!(plan.into_request(None).unwrap().native, requested);
    }
    #[test]
    fn actual_asio_switch_capability_roundtrips_clock_view_and_inactive_wasapi_fields() {
        let bounds = AsioClockBounds {
            timer: 10,
            drift: 20,
            latency: 30,
            age: 1_000_000_000,
        };
        let cap = asio_capability(
            DRIVER,
            beatkernel_platform::audio::asio::AsioBufferRequest::Frames(64),
            &[0, 1],
            None,
        )
        .unwrap();
        let cap = switch_capability(
            asio_clock_capability(cap, bounds).unwrap(),
            super::super::Backend::Asio,
            Some(super::super::AsioView::Bits64),
        )
        .unwrap();
        let fields = cap.settings().unwrap();
        assert_eq!(
            fields
                .fields()
                .iter()
                .find(|f| f.flag == "--device")
                .unwrap()
                .label,
            "TRUSTED ASIO DRIVER CLSID"
        );
        for flag in ["--mode", "--period", "--shared-policy"] {
            assert!(fields
                .fields()
                .iter()
                .find(|f| f.flag == flag)
                .unwrap()
                .value
                .is_empty());
        }
        let TargetPlan::Asio(plan) = plan_target(
            &cap,
            None,
            AudioFormat::new(48_000, 2).unwrap(),
            None,
            &cap.current_args,
            true,
        )
        .unwrap() else {
            panic!("ASIO capability changed target")
        };
        assert_eq!(plan.bounds, bounds);
        assert_eq!(plan.view, super::super::AsioView::Bits64);
        assert_eq!(plan.channels, [0, 1]);
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
            assert!(request(
                current(),
                None,
                &args.into_iter().map(str::to_owned).collect::<Vec<_>>()
            )
            .is_err());
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
        assert!(request(
            shared.clone(),
            None,
            &["--period".into(), "frames:32".into()]
        )
        .is_err());
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
            assert!(asio_request(
                DRIVER,
                AsioBufferRequest::DriverPreferred,
                &[0, 1],
                None,
                &args.into_iter().map(str::to_owned).collect::<Vec<_>>()
            )
            .is_err());
        }
        assert!(asio_capability(DRIVER, AsioBufferRequest::Frames(0), &[0, 1], None).is_err());
        assert!(asio_capability(DRIVER, AsioBufferRequest::DriverPreferred, &[], None).is_err());
        assert!(
            asio_capability(DRIVER, AsioBufferRequest::DriverPreferred, &[0, 0], None).is_err()
        );
        assert!(asio_capability(
            DRIVER,
            AsioBufferRequest::DriverPreferred,
            &[u32::MAX],
            None
        )
        .is_err());
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
        assert!(asio_request(
            DRIVER,
            buffer,
            &reordered,
            None,
            &["--output-channels".into(), "7,4,2".into()]
        )
        .is_err());
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
        assert!(asio_request(
            DRIVER,
            buffer,
            &resized,
            matrix.as_ref(),
            &["--output-matrix".into(), "exact".into()]
        )
        .is_err());
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
                assert!(asio_request(
                    DRIVER,
                    AsioBufferRequest::DriverPreferred,
                    &[0, 1],
                    None,
                    &["--output-channels".into(), text.into()]
                )
                .is_err());
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
        assert!(request(
            engine.native.clone(),
            None,
            &["--shared-policy".into(), "legacy".into()]
        )
        .is_err());
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
            assert!(request(
                original.clone(),
                None,
                &args.into_iter().map(str::to_owned).collect::<Vec<_>>()
            )
            .is_err());
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

    #[test]
    fn live_clock_bounds_parse_preserve_and_roundtrip_as_eight_advertised_fields() {
        use beatkernel_platform::audio::asio::AsioBufferRequest;
        let current = AsioClockBounds {
            timer: 10,
            drift: 20,
            latency: 30,
            age: 1_000_000_000,
        };
        assert_eq!(asio_clock_request(current, &[]).unwrap(), current);
        let changed = asio_clock_request(
            current,
            &[
                "--asio-timer-error-ns".into(),
                "0".into(),
                "--asio-latency-error-ns".into(),
                "200".into(),
                "--asio-anchor-age-ns".into(),
                "1000000".into(),
            ],
        )
        .unwrap();
        assert_eq!(
            changed,
            AsioClockBounds {
                timer: 0,
                drift: 20,
                latency: 200,
                age: 1_000_000
            }
        );
        let cap = asio_clock_capability(
            asio_capability(DRIVER, AsioBufferRequest::Frames(64), &[0, 1], None).unwrap(),
            changed,
        )
        .unwrap();
        assert_eq!(cap.settings().unwrap().fields().len(), 8);
        assert_eq!(
            asio_clock_request(current, &cap.current_args).unwrap(),
            changed
        );
        assert!(request(
            self::current(),
            None,
            &["--asio-timer-error-ns".into(), "0".into()]
        )
        .is_err());
    }

    #[test]
    fn malformed_clock_bounds_and_ambiguous_horizons_refuse_before_native_io() {
        let current = AsioClockBounds {
            timer: 0,
            drift: 0,
            latency: 0,
            age: 1000,
        };
        for args in [
            vec!["--asio-anchor-age-ns", "0"],
            vec!["--asio-anchor-age-ns", "2147483648000000"],
            vec!["--asio-timer-error-ns", "9223372036854775808"],
            vec!["--asio-drift-error-ns", "+1"],
            vec!["--asio-latency-error-ns", "-1"],
            vec!["--asio-timer-error-ns", "2147483648000000"],
        ] {
            assert!(asio_clock_request(
                current,
                &args.into_iter().map(str::to_owned).collect::<Vec<_>>()
            )
            .is_err());
        }
    }
}
