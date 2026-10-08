//! Pure converted settings admission and applied native metadata; no device execution.
use super::*;
use beatkernel::time::{ClockDomainId, Timestamp};
use beatkernel_platform::audio::SampleEncoding;

fn current() -> AlsaRequest {
    AlsaRequest {
        device: "null".into(),
        format: DeviceFormat::new(48_000, 2, SampleEncoding::Float32, None).unwrap(),
        period_frames: 64,
        buffer_frames: 256,
        allow_size_rounding: false,
        monotonic_domain: ClockDomainId(1),
    }
}
fn matrix() -> ChannelMatrix {
    ChannelMatrix::new(1, 2, &[1.0, 0.5]).unwrap()
}
fn args(pairs: &[(&str, &str)]) -> Vec<String> {
    pairs
        .iter()
        .flat_map(|(flag, value)| [(*flag).into(), (*value).into()])
        .collect()
}
fn applied() -> AlsaAppliedConfig {
    let native = current();
    AlsaAppliedConfig {
        format: native.format,
        buffer_frames: native.buffer_frames,
        period_frames: native.period_frames,
        requested: native,
        sizing_adjusted: false,
        output_domain: ClockDomainId(2),
        output_origin: Timestamp::ZERO,
    }
}
fn field(cap: &OutputCapability, flag: &str) -> String {
    cap.settings()
        .unwrap()
        .fields()
        .iter()
        .find(|f| f.flag == flag)
        .unwrap()
        .value
        .clone()
}

#[test]
fn converted_settings_map_target_rate_period_buffer_and_matrix_without_changing_source_dimensions()
{
    let native = current();
    let retained = matrix();
    let request = converted_request_for_args(
        &native,
        &retained,
        &args(&[
            ("--rate", "32000"),
            ("--period-frames", "32"),
            ("--buffer-frames", "128"),
            ("--output-matrix", "0.25;0.75;1"),
        ]),
    )
    .unwrap();
    assert_eq!(request.native.format.sample_rate(), 32_000);
    assert_eq!(request.native.format.channels(), 3);
    assert_eq!(
        (request.native.period_frames, request.native.buffer_frames),
        (32, 128)
    );
    let changed = request.matrix.as_ref().unwrap();
    assert_eq!(
        (changed.source_channels(), changed.target_channels()),
        (1, 3)
    );
    assert_eq!(matrix_text(changed).unwrap(), "0.25;0.75;1");
    assert_eq!(request.native.device, native.device);
    assert_eq!(request.native.monotonic_domain, native.monotonic_domain);
    assert_eq!(
        request.native.allow_size_rounding,
        native.allow_size_rounding
    );
    let blank = converted_request_for_args(
        &request.native,
        changed,
        &args(&[
            ("--rate", ""),
            ("--output-matrix", ""),
            ("--period-frames", ""),
        ]),
    )
    .unwrap();
    assert_eq!(blank.native, request.native);
    assert_eq!(blank.matrix.as_ref(), Some(changed));
    let exact = converted_request_for_args(
        &request.native,
        changed,
        &args(&[("--output-matrix", "exact")]),
    )
    .unwrap();
    assert_eq!(exact.native.format.sample_rate(), 32_000);
    assert_eq!(exact.native.format.channels(), 1);
    assert_eq!(
        exact.matrix.as_ref().unwrap(),
        &ChannelMatrix::default_mix(1, 1).unwrap()
    );
    assert_eq!(native, current());
    assert_eq!(retained, matrix());
}

#[test]
fn converted_parser_errors_refuse_before_mutating_native_request_or_retained_matrix() {
    let native = current();
    let retained = matrix();
    for pairs in [
        vec![("--rate", "0")],
        vec![("--rate", "-1")],
        vec![("--rate", "48000.0")],
        vec![("--rate", "4294967296")],
        vec![("--period-frames", "0")],
        vec![("--period-frames", "256")],
        vec![("--buffer-frames", "64")],
        vec![("--output-matrix", "NaN")],
        vec![("--output-matrix", "1,0")],
        vec![("--output-matrix", "1;")],
        vec![("--rate", "32000"), ("--rate", "48000")],
        vec![("--channels", "2")],
    ] {
        assert!(
            converted_request_for_args(&native, &retained, &args(&pairs)).is_err(),
            "{pairs:?}"
        );
        assert_eq!(native, current());
        assert_eq!(retained, matrix());
    }
    assert!(converted_request_for_args(&native, &retained, &["--rate".into()]).is_err());
    assert!(
        converted_request_for_args(&native, &ChannelMatrix::default_mix(1, 1).unwrap(), &[])
            .is_err()
    );
}

#[test]
fn converted_capability_advertises_actual_target_format_and_authorized_applied_sizing() {
    let mut metadata = applied();
    metadata.requested.allow_size_rounding = true;
    metadata.buffer_frames = 512;
    metadata.period_frames = 128;
    metadata.sizing_adjusted = true;
    let cap = converted_capability(&metadata, &matrix()).unwrap();
    assert_eq!(field(&cap, "--rate"), "48000");
    assert_eq!(field(&cap, "--period-frames"), "128");
    assert_eq!(field(&cap, "--buffer-frames"), "512");
    assert_eq!(field(&cap, "--output-matrix"), "1;0.5");
    let next =
        converted_request_for_args(&metadata.requested, &matrix(), &cap.current_args).unwrap();
    assert_eq!(next.native.format, metadata.format);
    assert_eq!(
        (next.native.period_frames, next.native.buffer_frames),
        (128, 512)
    );
    assert_eq!(next.matrix.as_ref(), Some(&matrix()));
    metadata.sizing_adjusted = false;
    assert!(converted_capability(&metadata, &matrix()).is_err());
    metadata.sizing_adjusted = true;
    metadata.requested.allow_size_rounding = false;
    assert!(converted_capability(&metadata, &matrix()).is_err());
    metadata = applied();
    metadata.format = DeviceFormat::new(32_000, 2, SampleEncoding::Float32, None).unwrap();
    assert!(converted_capability(&metadata, &matrix()).is_err());
    metadata = applied();
    assert!(converted_capability(&metadata, &ChannelMatrix::default_mix(1, 1).unwrap()).is_err());
    metadata.period_frames = 0;
    assert!(converted_capability(&metadata, &matrix()).is_err());
}

#[test]
fn legacy_alsa_capability_omits_rate_editor_and_refuses_explicit_rate_before_mapping() {
    let legacy = capability(&applied()).unwrap();
    assert!(!legacy
        .settings()
        .unwrap()
        .fields()
        .iter()
        .any(|f| f.flag == "--rate"));
    let native = current();
    for value in ["32000", ""] {
        let request = args(&[("--rate", value)]);
        assert!(request_for_args(&native, &request).is_err());
        assert!(remixed_request_for_args(&native, Some(&matrix()), &request).is_err());
    }
    assert_eq!(native, current());
}
