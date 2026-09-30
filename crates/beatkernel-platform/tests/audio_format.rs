use beatkernel::time::Duration;
use beatkernel_platform::audio::*;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<[usize; 3]> = const { Cell::new([0; 3]) };
}
struct Allocator;
fn allocation(kind: usize) {
    let _ = TRACK.try_with(|track| {
        if track.get() {
            let _ = COUNTS.try_with(|counts| {
                let mut value = counts.get();
                value[kind] += 1;
                counts.set(value);
            });
        }
    });
}
// SAFETY: System receives the original valid allocator arguments; thread-local
// counters neither allocate nor alter allocation lifetimes or pointers.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        allocation(0);
        // SAFETY: The caller provides a valid allocation layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        allocation(0);
        // SAFETY: The caller provides a valid allocation layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        allocation(1);
        // SAFETY: Caller provides a live System allocation and matching layout.
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        allocation(2);
        // SAFETY: Caller provides the original live allocation and layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn tracked<T>(operation: impl FnOnce() -> T) -> (T, [usize; 3]) {
    COUNTS.with(|c| c.set([0; 3]));
    TRACK.with(|t| t.set(true));
    let result = operation();
    TRACK.with(|t| t.set(false));
    (result, COUNTS.with(Cell::get))
}
fn pcm(container_bits: u16, valid_bits: u16) -> DeviceFormat {
    DeviceFormat::new(
        48_000,
        1,
        SampleEncoding::Pcm {
            container_bits,
            valid_bits,
        },
        Some(0),
    )
    .unwrap()
}
fn dur(ns: i64) -> Duration {
    Duration::from_nanos(ns)
}
fn request(
    mode: AudioStreamMode,
    buffer: BufferRequest,
    period: PeriodRequest,
) -> AudioStreamRequest {
    AudioStreamRequest::new(
        AudioDeviceId("explicit-device".into()),
        AudioBackendKind::Wasapi,
        mode,
        pcm(16, 16),
        buffer,
        period,
    )
    .unwrap()
}
fn engine(buffer: BufferRequest, period: PeriodRequest) -> AudioStreamRequest {
    request(
        AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod),
        buffer,
        period,
    )
}
fn constraints() -> PeriodConstraints {
    PeriodConstraints {
        default_frames: Some(240),
        default_period: Some(dur(5_000_000)),
        min_frames: Some(96),
        max_frames: Some(960),
        fundamental_frames: Some(48),
        alignment_frames: Some(128),
        ..PeriodConstraints::default()
    }
}
fn assert_packing(format: DeviceFormat, input: &[f32], expected: &[u8]) {
    let mut output = vec![0xa5; expected.len()];
    encode_pcm(format, input, &mut output).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn signed_pcm_packing_clamps_and_rounds_ties_away_from_zero() {
    assert_packing(
        pcm(16, 16),
        &[
            -2.0,
            -1.0,
            -0.5,
            -1.0 / 65536.0,
            0.0,
            1.0 / 65536.0,
            0.5,
            1.0,
            2.0,
        ],
        &[
            0, 128, 0, 128, 0, 192, 255, 255, 0, 0, 1, 0, 0, 64, 255, 127, 255, 127,
        ],
    );
    assert_packing(
        pcm(24, 24),
        &[-1.0, -0.5, 0.0, 0.5, 1.0],
        &[0, 0, 128, 0, 0, 192, 0, 0, 0, 0, 0, 64, 255, 255, 127],
    );
    assert_packing(
        pcm(32, 32),
        &[-1.0, -0.5, 0.0, 0.5, 1.0],
        &[
            0, 0, 0, 128, 0, 0, 0, 192, 0, 0, 0, 0, 0, 0, 0, 64, 255, 255, 255, 127,
        ],
    );
}
#[test]
fn narrow_valid_bits_are_saturated_and_left_aligned() {
    assert_packing(
        pcm(24, 20),
        &[-1.0, -0.5, -1.0 / 1048576.0, 0.0, 1.0 / 1048576.0, 0.5, 1.0],
        &[
            0, 0, 128, 0, 0, 192, 240, 255, 255, 0, 0, 0, 16, 0, 0, 0, 0, 64, 240, 255, 127,
        ],
    );
    assert_packing(
        pcm(32, 24),
        &[
            -1.0,
            -0.5,
            -1.0 / 16777216.0,
            0.0,
            1.0 / 16777216.0,
            0.5,
            1.0,
        ],
        &[
            0, 0, 0, 128, 0, 0, 0, 192, 0, 255, 255, 255, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 64, 0,
            255, 255, 127,
        ],
    );
    assert_packing(
        pcm(16, 1),
        &[-1.0, -0.5, 0.0, 0.5, 1.0],
        &[0, 128, 0, 128, 0, 0, 0, 0, 0, 0],
    );
}
#[test]
fn finite_float_output_preserves_channel_order_signed_zero_and_unclamped_values() {
    let format = DeviceFormat::new(12_345, 2, SampleEncoding::Float32, Some(0)).unwrap();
    assert_packing(
        format,
        &[0.0, -0.0, 2.0, -3.25],
        &[0, 0, 0, 0, 0, 0, 0, 128, 0, 0, 0, 64, 0, 0, 80, 192],
    );
    let format = DeviceFormat::new(
        48_000,
        2,
        SampleEncoding::Pcm {
            container_bits: 16,
            valid_bits: 16,
        },
        Some(3),
    )
    .unwrap();
    assert_packing(
        format,
        &[-1.0, 0.5, 0.25, -0.5],
        &[0, 128, 0, 64, 0, 32, 0, 192],
    );
}
#[test]
fn conversion_validates_every_value_and_exact_frame_extent_before_writing() {
    for encoding in [
        SampleEncoding::Float32,
        SampleEncoding::Pcm {
            container_bits: 24,
            valid_bits: 20,
        },
    ] {
        let format = DeviceFormat::new(48_000, 2, encoding, Some(3)).unwrap();
        let extent = usize::from(format.block_align());
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut output = vec![0xa5; extent];
            assert_eq!(
                encode_pcm(format, &[0.25, bad], &mut output),
                Err(AudioPlatformError::InvalidFormat)
            );
            assert_eq!(output, vec![0xa5; extent]);
        }
        for length in [0, extent - 1, extent + 1, extent * 2] {
            let mut output = vec![0xa5; length];
            assert_eq!(
                encode_pcm(format, &[0.25, -0.5], &mut output),
                Err(AudioPlatformError::InvalidFormat)
            );
            assert_eq!(output, vec![0xa5; length]);
        }
        let mut output = vec![0xa5; usize::from(encoding.bytes_per_sample())];
        assert_eq!(
            encode_pcm(format, &[0.25], &mut output),
            Err(AudioPlatformError::InvalidFormat)
        );
        assert!(output.iter().all(|byte| *byte == 0xa5));
        encode_pcm(format, &[], &mut []).unwrap();
    }
}
#[test]
fn native_formats_validate_width_mask_byte_rate_without_rate_presets() {
    let format = DeviceFormat::new(
        12_345,
        3,
        SampleEncoding::Pcm {
            container_bits: 24,
            valid_bits: 20,
        },
        Some(7),
    )
    .unwrap();
    assert_eq!(format.sample_rate(), 12_345);
    assert_eq!(format.channels(), 3);
    assert_eq!(format.block_align(), 9);
    assert_eq!(format.bytes_per_second(), 111_105);
    assert_eq!(format.channel_mask(), Some(7));
    assert!(DeviceFormat::new(48_000, 8, SampleEncoding::Float32, Some(0)).is_ok());
    assert!(DeviceFormat::new(48_000, 2, SampleEncoding::Float32, None).is_ok());
    for (rate, channels, encoding, mask) in [
        (0, 1, SampleEncoding::Float32, None),
        (48_000, 0, SampleEncoding::Float32, None),
        (48_000, 33, SampleEncoding::Float32, None),
        (u32::MAX, 2, SampleEncoding::Float32, None),
        (48_000, 2, SampleEncoding::Float32, Some(1)),
        (
            48_000,
            1,
            SampleEncoding::Pcm {
                container_bits: 8,
                valid_bits: 8,
            },
            None,
        ),
        (
            48_000,
            1,
            SampleEncoding::Pcm {
                container_bits: 24,
                valid_bits: 0,
            },
            None,
        ),
        (
            48_000,
            1,
            SampleEncoding::Pcm {
                container_bits: 24,
                valid_bits: 25,
            },
            None,
        ),
    ] {
        assert_eq!(
            DeviceFormat::new(rate, channels, encoding, mask),
            Err(AudioPlatformError::InvalidFormat)
        );
    }
}
#[test]
fn request_preserves_explicit_configuration_and_rejects_nul_or_nonpositive_sizes() {
    let mode = AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod);
    let format = DeviceFormat::new(12_345, 3, SampleEncoding::Float32, Some(0)).unwrap();
    let valid = AudioStreamRequest::new(
        AudioDeviceId("arbitrary".into()),
        AudioBackendKind::Wasapi,
        mode,
        format,
        BufferRequest::Frames(123),
        PeriodRequest::Frames(17),
    )
    .unwrap();
    assert_eq!(valid.format(), format);
    assert_eq!(valid.mode(), mode);
    assert_eq!(valid.negotiation(), NegotiationPolicy::Exact);
    assert_eq!(valid.buffer(), BufferRequest::Frames(123));
    assert_eq!(valid.period(), PeriodRequest::Frames(17));
    for device in ["", "bad\0identity"] {
        assert_eq!(
            AudioStreamRequest::new(
                AudioDeviceId(device.into()),
                AudioBackendKind::Wasapi,
                mode,
                format,
                BufferRequest::DeviceDefault,
                PeriodRequest::DeviceDefault
            ),
            Err(AudioPlatformError::InvalidRequest)
        );
    }
    for buffer in [
        BufferRequest::Frames(0),
        BufferRequest::Duration(dur(0)),
        BufferRequest::Duration(dur(-1)),
    ] {
        assert_eq!(
            AudioStreamRequest::new(
                AudioDeviceId("a".into()),
                AudioBackendKind::Wasapi,
                mode,
                format,
                buffer,
                PeriodRequest::DeviceDefault
            ),
            Err(AudioPlatformError::InvalidRequest)
        );
    }
    for period in [
        PeriodRequest::Frames(0),
        PeriodRequest::Duration(dur(0)),
        PeriodRequest::Duration(dur(-1)),
    ] {
        assert_eq!(
            AudioStreamRequest::new(
                AudioDeviceId("a".into()),
                AudioBackendKind::Wasapi,
                mode,
                format,
                BufferRequest::DeviceDefault,
                period
            ),
            Err(AudioPlatformError::InvalidRequest)
        );
    }
}
#[test]
fn engine_exact_periods_and_defaults_keep_native_bounds_and_report_rounding() {
    let native = constraints();
    for period in [
        PeriodRequest::Frames(240),
        PeriodRequest::Duration(dur(5_000_000)),
        PeriodRequest::DeviceDefault,
    ] {
        let resolved =
            resolve_period(&engine(BufferRequest::DeviceDefault, period), native).unwrap();
        assert_eq!(resolved.frames, 240);
        assert_eq!(resolved.duration, dur(5_000_000));
        assert!(!resolved.adjusted);
    }
    for (requested, suggested) in [(1, 96), (97, 144), (961, 960)] {
        let exact = engine(
            BufferRequest::DeviceDefault,
            PeriodRequest::Frames(requested),
        );
        assert!(
            matches!(resolve_period(&exact, native), Err(AudioPlatformError::ConfigurationUnsupported { constraint:ConfigurationConstraint::PeriodBounds, constraints, suggested_period_frames:Some(n), .. }) if constraints == native && n == suggested)
        );
        let resolved = resolve_period(
            &exact.with_negotiation(NegotiationPolicy::AllowSupportedRounding),
            native,
        )
        .unwrap();
        assert_eq!(resolved.frames, suggested);
        assert!(resolved.adjusted);
    }
}
#[test]
fn duration_exactness_is_rational_and_opt_in_rounds_to_supported_multiple() {
    let native = constraints();
    let exact = engine(
        BufferRequest::DeviceDefault,
        PeriodRequest::Duration(dur(2_000_001)),
    );
    assert!(matches!(
        resolve_period(&exact, native),
        Err(AudioPlatformError::ConfigurationUnsupported {
            suggested_period_frames: Some(144),
            ..
        })
    ));
    let resolved = resolve_period(
        &exact.with_negotiation(NegotiationPolicy::AllowSupportedRounding),
        native,
    )
    .unwrap();
    assert_eq!(resolved.frames, 144);
    assert_eq!(resolved.duration, dur(3_000_000));
    assert!(resolved.adjusted);
    let exact = engine(
        BufferRequest::Duration(dur(5_000_000)),
        PeriodRequest::Frames(240),
    );
    assert_eq!(validate_buffer_size(&exact, 240), Ok(false));
    let fractional = engine(
        BufferRequest::Duration(dur(5_000_001)),
        PeriodRequest::Frames(240),
    );
    assert!(validate_buffer_size(&fractional, 241).is_err());
    assert_eq!(
        validate_buffer_size(
            &fractional.with_negotiation(NegotiationPolicy::AllowSupportedRounding),
            241
        ),
        Ok(true)
    );
}
#[test]
fn buffers_are_independent_and_device_default_does_not_change_exact_period() {
    let exact = engine(BufferRequest::Frames(512), PeriodRequest::Frames(240));
    assert_eq!(validate_buffer_size(&exact, 512), Ok(false));
    assert!(matches!(
        validate_buffer_size(&exact, 480),
        Err(AudioPlatformError::ConfigurationUnsupported {
            suggested_buffer_frames: Some(480),
            ..
        })
    ));
    assert_eq!(
        validate_buffer_size(
            &exact.with_negotiation(NegotiationPolicy::AllowSupportedRounding),
            480
        ),
        Ok(true)
    );
    let default = engine(BufferRequest::DeviceDefault, PeriodRequest::Frames(240));
    assert_eq!(validate_buffer_size(&default, 512), Ok(false));
    assert_eq!(resolve_period(&default, constraints()).unwrap().frames, 240);
}
#[test]
fn legacy_requires_explicit_default_and_exclusive_requires_matching_sizes() {
    let legacy = request(
        AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault),
        BufferRequest::Frames(512),
        PeriodRequest::Frames(240),
    );
    assert!(matches!(
        resolve_period(&legacy, constraints()),
        Err(AudioPlatformError::ConfigurationUnsupported {
            constraint: ConfigurationConstraint::LegacyDeviceDefaultPeriod,
            ..
        })
    ));
    let legacy = request(
        AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault),
        BufferRequest::Frames(512),
        PeriodRequest::DeviceDefault,
    );
    assert_eq!(resolve_period(&legacy, constraints()).unwrap().frames, 240);
    let exclusive = request(
        AudioStreamMode::Exclusive,
        BufferRequest::Frames(192),
        PeriodRequest::Frames(240),
    );
    let exclusive_constraints = PeriodConstraints {
        alignment_frames: None,
        ..constraints()
    };
    assert!(matches!(
        resolve_period(&exclusive, exclusive_constraints),
        Err(AudioPlatformError::ConfigurationUnsupported {
            constraint: ConfigurationConstraint::ExclusiveBufferEqualsPeriod,
            ..
        })
    ));
    let exclusive = request(
        AudioStreamMode::Exclusive,
        BufferRequest::Frames(240),
        PeriodRequest::Frames(240),
    );
    assert_eq!(
        resolve_period(&exclusive, exclusive_constraints)
            .unwrap()
            .frames,
        240
    );
    assert_eq!(exclusive.mode(), AudioStreamMode::Exclusive);
    assert!(matches!(
        resolve_period(&exclusive, constraints()),
        Err(AudioPlatformError::ConfigurationUnsupported {
            suggested_period_frames: Some(384),
            suggested_buffer_frames: Some(384),
            ..
        })
    ));
    let resolved = resolve_period(
        &exclusive.with_negotiation(NegotiationPolicy::AllowSupportedRounding),
        constraints(),
    )
    .unwrap();
    assert_eq!(resolved.frames, 384);
    assert_eq!(resolved.duration, dur(8_000_000));
    assert!(resolved.adjusted);
}
#[test]
fn duration_only_native_period_bounds_default_and_missing_evidence_are_explicit() {
    let native = PeriodConstraints {
        default_period: Some(dur(5_000_000)),
        min_period: Some(dur(2_000_001)),
        max_period: Some(dur(5_000_001)),
        ..PeriodConstraints::default()
    };
    let default = resolve_period(
        &engine(BufferRequest::DeviceDefault, PeriodRequest::DeviceDefault),
        native,
    )
    .unwrap();
    assert_eq!(default.frames, 240);
    assert_eq!(default.duration, dur(5_000_000));
    assert!(!default.adjusted);
    for (wanted, suggestion) in [(96, 97), (241, 240)] {
        let request = engine(BufferRequest::DeviceDefault, PeriodRequest::Frames(wanted));
        assert!(
            matches!(resolve_period(&request,native),Err(AudioPlatformError::ConfigurationUnsupported {suggested_period_frames:Some(actual),..}) if actual==suggestion)
        );
        assert_eq!(
            resolve_period(
                &request.with_negotiation(NegotiationPolicy::AllowSupportedRounding),
                native
            )
            .unwrap()
            .frames,
            suggestion
        );
    }
    assert!(matches!(
        resolve_period(
            &engine(BufferRequest::DeviceDefault, PeriodRequest::DeviceDefault),
            PeriodConstraints::default()
        ),
        Err(AudioPlatformError::ConfigurationUnsupported {
            suggested_period_frames: None,
            ..
        })
    ));
}

#[test]
fn impossible_or_overflowing_period_constraints_do_not_invent_supported_suggestions() {
    let selected = engine(BufferRequest::DeviceDefault, PeriodRequest::Frames(240));
    for native in [
        PeriodConstraints {
            fundamental_frames: Some(0),
            ..PeriodConstraints::default()
        },
        PeriodConstraints {
            min_frames: Some(10),
            max_frames: Some(9),
            ..PeriodConstraints::default()
        },
        PeriodConstraints {
            min_frames: Some(97),
            max_frames: Some(100),
            fundamental_frames: Some(48),
            ..PeriodConstraints::default()
        },
    ] {
        assert!(matches!(
            resolve_period(&selected, native),
            Err(AudioPlatformError::ConfigurationUnsupported {
                constraint: ConfigurationConstraint::PeriodBounds,
                suggested_period_frames: None,
                ..
            })
        ));
    }
    let exclusive = request(
        AudioStreamMode::Exclusive,
        BufferRequest::Frames(240),
        PeriodRequest::Frames(240),
    );
    for native in [
        PeriodConstraints {
            alignment_frames: Some(0),
            ..PeriodConstraints::default()
        },
        PeriodConstraints {
            fundamental_frames: Some(u32::MAX),
            alignment_frames: Some(u32::MAX - 1),
            ..PeriodConstraints::default()
        },
    ] {
        assert!(matches!(
            resolve_period(&exclusive, native),
            Err(AudioPlatformError::ConfigurationUnsupported {
                suggested_period_frames: None,
                ..
            })
        ));
    }
    let huge = engine(
        BufferRequest::DeviceDefault,
        PeriodRequest::Duration(Duration::MAX),
    );
    assert_eq!(
        resolve_period(&huge, PeriodConstraints::default()),
        Err(AudioPlatformError::InvalidRequest)
    );
}

#[test]
fn actual_buffer_zero_and_mode_specific_mismatches_have_precise_classifications() {
    for (mode, constraint) in [
        (
            AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod),
            ConfigurationConstraint::EngineManagedBuffer,
        ),
        (
            AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault),
            ConfigurationConstraint::BufferSize,
        ),
        (
            AudioStreamMode::Exclusive,
            ConfigurationConstraint::BufferAlignment,
        ),
    ] {
        let selected = request(
            mode,
            BufferRequest::Frames(512),
            PeriodRequest::DeviceDefault,
        );
        assert_eq!(
            validate_buffer_size(&selected, 0),
            Err(AudioPlatformError::InvalidRequest)
        );
        assert_eq!(
            validate_buffer_size(&selected, 480),
            Err(AudioPlatformError::ConfigurationUnsupported {
                constraint,
                constraints: PeriodConstraints::default(),
                suggested_buffer_frames: Some(480),
                suggested_period_frames: None,
            })
        );
    }
}

#[test]
// Fixed-size platform errors preserve the allocation-free conversion boundary;
// boxing them would invalidate this test's zero-allocation error-path oracle.
#[allow(clippy::result_large_err)]
fn converter_allocation_measurement_is_calibrated_and_success_and_errors_are_rt_safe() {
    let (_, counts) = tracked(|| {
        let mut bytes = Vec::with_capacity(1);
        bytes.push(1u8);
        bytes.reserve_exact(1024);
        std::hint::black_box(&bytes);
        drop(bytes);
    });
    assert!(counts.iter().all(|count| *count > 0));
    let format = pcm(24, 20);
    let mut output = [0xa5; 6];
    let (result, counts) = tracked(|| encode_pcm(format, &[-0.5, 0.5], &mut output));
    assert_eq!(counts, [0; 3]);
    assert!(result.is_ok());
    assert_eq!(output, [0, 0, 192, 0, 0, 64]);
    let previous = output;
    let (result, counts) = tracked(|| encode_pcm(format, &[0.5, f32::NAN], &mut output));
    assert_eq!(counts, [0; 3]);
    assert!(result.is_err());
    assert_eq!(output, previous);
    let (result, counts) = tracked(|| encode_pcm(format, &[0.5], &mut output));
    assert_eq!(counts, [0; 3]);
    assert!(result.is_err());
    assert_eq!(output, previous);
    let float = DeviceFormat::new(48_000, 2, SampleEncoding::Float32, Some(0)).unwrap();
    let mut float_output = [0xa5; 8];
    let (result, counts) = tracked(|| encode_pcm(float, &[-0.0, 2.0], &mut float_output));
    assert_eq!(counts, [0; 3]);
    assert!(result.is_ok());
    assert_eq!(float_output, [0, 0, 0, 128, 0, 0, 0, 64]);
}
