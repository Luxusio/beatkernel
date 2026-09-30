//! SDK-independent ASIO negotiation fixtures, authored without native calls.
use beatkernel_platform::audio::asio::{
    AsioBufferConstraints, AsioBufferRequest, AsioConfigurationError, AsioSampleRateRequest,
};
use std::error::Error;

#[test]
fn zero_granularity_variable_range_accepts_each_exact_integral_request() {
    let constraints = AsioBufferConstraints::from_raw(63, 1025, 250, 0).unwrap();
    assert_eq!(constraints.min_frames(), 63);
    assert_eq!(constraints.max_frames(), 1025);
    assert_eq!(constraints.preferred_frames(), 250);
    assert_eq!(constraints.granularity(), 0);
    for frames in [63, 64, 127, 250, 251, 1024, 1025] {
        assert!(constraints.supports(frames));
        assert_eq!(
            constraints.resolve(AsioBufferRequest::Frames(frames)),
            Ok(frames)
        );
    }
    assert_eq!(
        constraints.resolve(AsioBufferRequest::DriverPreferred),
        Ok(250)
    );
    for frames in [0, 62, 1026, u32::MAX] {
        assert!(!constraints.supports(frames));
        assert_eq!(
            constraints.resolve(AsioBufferRequest::Frames(frames)),
            Err(AsioConfigurationError::UnsupportedBuffer { requested: frames })
        );
    }
}

#[test]
fn equal_bounds_are_fixed_and_require_zero_granularity() {
    let constraints = AsioBufferConstraints::from_raw(256, 256, 256, 0).unwrap();
    assert!(constraints.supports(256));
    assert_eq!(
        constraints.resolve(AsioBufferRequest::DriverPreferred),
        Ok(256)
    );
    for frames in [0, 255, 257, 512] {
        assert_eq!(
            constraints.resolve(AsioBufferRequest::Frames(frames)),
            Err(AsioConfigurationError::UnsupportedBuffer { requested: frames })
        );
    }
    for granularity in [-1, 1, 256] {
        assert_eq!(
            AsioBufferConstraints::from_raw(256, 256, 256, granularity),
            Err(AsioConfigurationError::InvalidDriverReport)
        );
    }
}

#[test]
fn positive_granularity_is_min_anchored_and_does_not_round_or_require_aligned_maximum() {
    let constraints = AsioBufferConstraints::from_raw(96, 300, 160, 32).unwrap();
    assert_eq!(constraints.granularity(), 32);
    for frames in [96, 128, 160, 192, 224, 256, 288] {
        assert_eq!(
            constraints.resolve(AsioBufferRequest::Frames(frames)),
            Ok(frames)
        );
    }
    for frames in [95, 97, 159, 161, 299, 300, 320] {
        assert!(!constraints.supports(frames));
        assert_eq!(
            constraints.resolve(AsioBufferRequest::Frames(frames)),
            Err(AsioConfigurationError::UnsupportedBuffer { requested: frames })
        );
    }
    assert_eq!(
        AsioBufferConstraints::from_raw(96, 300, 300, 32),
        Err(AsioConfigurationError::InvalidDriverReport)
    );
    let offset = AsioBufferConstraints::from_raw(65, 200, 129, 32).unwrap();
    assert!(offset.supports(65));
    assert!(offset.supports(97));
    assert!(offset.supports(129));
    assert!(!offset.supports(64));
    assert!(!offset.supports(128));
}

#[test]
fn minus_one_means_doubling_reported_minimum_even_when_minimum_is_not_power_of_two() {
    let constraints = AsioBufferConstraints::from_raw(96, 1000, 384, -1).unwrap();
    assert_eq!(constraints.granularity(), -1);
    assert_eq!(
        constraints.resolve(AsioBufferRequest::DriverPreferred),
        Ok(384)
    );
    for frames in [96, 192, 384, 768] {
        assert!(constraints.supports(frames));
    }
    for frames in [0, 64, 128, 256, 512, 999, 1000, 1536] {
        assert_eq!(
            constraints.resolve(AsioBufferRequest::Frames(frames)),
            Err(AsioConfigurationError::UnsupportedBuffer { requested: frames })
        );
    }
    assert_eq!(
        AsioBufferConstraints::from_raw(96, 1000, 256, -1),
        Err(AsioConfigurationError::InvalidDriverReport)
    );
}

#[test]
fn near_i32_max_constraints_use_checked_exact_arithmetic() {
    let doubling =
        AsioBufferConstraints::from_raw(1_073_741_823, i32::MAX, 2_147_483_646, -1).unwrap();
    assert!(doubling.supports(1_073_741_823));
    assert!(doubling.supports(2_147_483_646));
    for frames in [2_147_483_645, i32::MAX as u32, u32::MAX] {
        assert!(!doubling.supports(frames));
    }
    assert_eq!(
        doubling.resolve(AsioBufferRequest::DriverPreferred),
        Ok(2_147_483_646)
    );
    let step = AsioBufferConstraints::from_raw(1, i32::MAX, i32::MAX, i32::MAX - 1).unwrap();
    assert!(step.supports(1));
    assert!(step.supports(i32::MAX as u32));
    assert!(!step.supports(i32::MAX as u32 - 1));
    assert!(!step.supports(u32::MAX));
    let variable = AsioBufferConstraints::from_raw(1, i32::MAX, i32::MAX, 0).unwrap();
    assert_eq!(
        variable.resolve(AsioBufferRequest::Frames(i32::MAX as u32)),
        Ok(i32::MAX as u32)
    );
    let fixed = AsioBufferConstraints::from_raw(i32::MAX, i32::MAX, i32::MAX, 0).unwrap();
    assert_eq!(
        fixed.resolve(AsioBufferRequest::DriverPreferred),
        Ok(i32::MAX as u32)
    );
}

#[test]
fn malformed_native_reports_reject_before_any_request_resolution() {
    for (minimum, maximum, preferred, granularity) in [
        (0, 512, 256, 0),
        (-1, 512, 256, 0),
        (64, 0, 64, 0),
        (64, -1, 64, 0),
        (512, 64, 256, 0),
        (64, 512, 0, 0),
        (64, 512, -1, 0),
        (64, 512, 63, 0),
        (64, 512, 513, 0),
        (64, 512, 256, -2),
        (64, 512, 256, i32::MIN),
        (128, 128, 127, 0),
        (128, 128, 129, 0),
        (65, 200, 128, 32),
        (96, 1000, 512, -1),
    ] {
        assert_eq!(
            AsioBufferConstraints::from_raw(minimum, maximum, preferred, granularity),
            Err(AsioConfigurationError::InvalidDriverReport),
            "report ({minimum},{maximum},{preferred},{granularity})"
        );
    }
}

#[test]
fn hertz_request_preserves_finite_positive_values_including_fractional_and_subnormal_rates() {
    for hertz in [44_100.0, 48_000.0, 44_100.25, f64::from_bits(1), f64::MAX] {
        assert_eq!(AsioSampleRateRequest::Hertz(hertz).validate(), Ok(()));
        let native = AsioSampleRateRequest::Hertz(hertz).native_value().unwrap();
        assert_eq!(native.to_bits(), hertz.to_bits());
    }
}

#[test]
fn only_explicit_external_clock_produces_native_zero_and_invalid_hertz_never_alias_it() {
    assert_eq!(AsioSampleRateRequest::ExternalClock.validate(), Ok(()));
    assert_eq!(
        AsioSampleRateRequest::ExternalClock
            .native_value()
            .unwrap()
            .to_bits(),
        0.0_f64.to_bits()
    );
    for hertz in [
        0.0,
        -0.0,
        -1.0,
        -44_100.0,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NAN,
        f64::from_bits(0x7ff8_0000_0000_0042),
    ] {
        assert_eq!(
            AsioSampleRateRequest::Hertz(hertz).validate(),
            Err(AsioConfigurationError::InvalidSampleRate)
        );
        assert_eq!(
            AsioSampleRateRequest::Hertz(hertz).native_value(),
            Err(AsioConfigurationError::InvalidSampleRate)
        );
    }
}

#[test]
fn errors_expose_unsupported_exact_request_and_implement_standard_error() {
    fn standard_error(error: &dyn Error) {
        assert!(!error.to_string().is_empty());
    }
    for error in [
        AsioConfigurationError::InvalidDriverReport,
        AsioConfigurationError::UnsupportedBuffer { requested: 513 },
        AsioConfigurationError::InvalidSampleRate,
    ] {
        standard_error(&error);
        assert!(!format!("{error:?}").is_empty());
    }
    assert!(AsioConfigurationError::UnsupportedBuffer { requested: 513 }
        .to_string()
        .contains("513"));
    assert_ne!(
        AsioConfigurationError::UnsupportedBuffer { requested: 513 },
        AsioConfigurationError::UnsupportedBuffer { requested: 514 }
    );
}
