//! Pure validation of native presentation metadata and explicit clock conversion.
use super::audio::{CoreAudioApplied, CoreAudioPresentation};
use beatkernel::time::{ClockDomainId, ClockMapper, ClockPair, ClockPoint, Timestamp};

/// Explicit conversion failure; no clock or configuration fallback is substituted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreAudioPresentationError {
    /// Checked frame-grid arithmetic cannot be represented as a timestamp.
    Overflow,
    /// A supplied point differs from its declared native/output domain.
    DomainMismatch {
        /// Required declared domain.
        expected: ClockDomainId,
        /// Supplied observation domain.
        received: ClockDomainId,
    },
    /// Exact request/applied configuration or native layout is inconsistent.
    InvalidConfiguration(&'static str),
    /// Supplied validity/sample/grid metadata is inconsistent.
    InvalidObservation(&'static str),
    /// The explicit mapper cannot supply the native-to-host relation.
    UnmappedHost {
        /// Native presentation domain.
        from: ClockDomainId,
        /// Required caller host domain.
        to: ClockDomainId,
    },
}
impl std::fmt::Display for CoreAudioPresentationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CoreAudio presentation: {self:?}")
    }
}
impl std::error::Error for CoreAudioPresentationError {}

/// Convert actual native presentation to an explicit output/host association.
///
/// No time is sampled here. Absent association returns None; contradictory
/// provided metadata rejects. Native sample-frame values remain diagnostic,
/// and native presentation can legitimately be later than userspace receipt.
pub fn coreaudio_presentation_pair(
    presentation: CoreAudioPresentation,
    applied: &CoreAudioApplied,
    host_domain: ClockDomainId,
    mapper: &dyn ClockMapper,
) -> Result<Option<ClockPair>, CoreAudioPresentationError> {
    use CoreAudioPresentationError as Error;
    if applied.buffer_channels.is_empty() || applied.buffer_channels.len() > 32 {
        return Err(Error::InvalidConfiguration(
            "native layout must contain 1..=32 buffers",
        ));
    }
    let layout_channels = applied
        .buffer_channels
        .iter()
        .try_fold(0u32, |sum, channels| sum.checked_add(*channels));
    if applied.request.device == 0
        || applied.buffer_frames == 0
        || applied.format != applied.request.format
        || applied.buffer_frames != applied.request.buffer_frames
        || applied.buffer_channels.contains(&0)
        || layout_channels != Some(u32::from(applied.format.channels()))
    {
        return Err(Error::InvalidConfiguration(
            "exact request/applied format, buffer and native layout required",
        ));
    }
    if presentation.frames == 0 || presentation.frames > applied.buffer_frames {
        return Err(Error::InvalidObservation(
            "presentation frames outside applied buffer",
        ));
    }
    let sample_valid = presentation.flags & 1 != 0;
    if sample_valid != presentation.native_sample_frame.is_some()
        || presentation
            .native_sample_frame
            .is_some_and(|value| !value.is_finite())
    {
        return Err(Error::InvalidObservation(
            "native sample-frame presence/finite value contradicts validity flag",
        ));
    }
    let host_valid = presentation.flags & 2 != 0;
    if !host_valid && presentation.native.is_some() {
        return Err(Error::InvalidObservation(
            "native host point provided without host validity flag",
        ));
    }
    if let Some(native) = presentation.native {
        if native.domain != applied.native_clock {
            return Err(Error::DomainMismatch {
                expected: applied.native_clock,
                received: native.domain,
            });
        }
    }
    if let Some(grid) = presentation.output_grid {
        if grid.domain != applied.output_domain {
            return Err(Error::DomainMismatch {
                expected: applied.output_domain,
                received: grid.domain,
            });
        }
        let offset = i128::from(presentation.first_frame)
            .checked_mul(1_000_000_000)
            .ok_or(Error::Overflow)?
            / i128::from(applied.format.sample_rate());
        let expected = i128::from(applied.output_origin.as_nanos())
            .checked_add(offset)
            .ok_or(Error::Overflow)?;
        let expected = Timestamp::from_nanos(i64::try_from(expected).map_err(|_| Error::Overflow)?);
        if grid.timestamp != expected {
            return Err(Error::InvalidObservation(
                "provided output grid does not match first-frame derivation",
            ));
        }
    }
    let (true, Some(native), Some(grid)) =
        (host_valid, presentation.native, presentation.output_grid)
    else {
        return Ok(None);
    };
    let target = mapper.map(native, host_domain).ok_or(Error::UnmappedHost {
        from: native.domain,
        to: host_domain,
    })?;
    Ok(Some(ClockPair {
        source: grid,
        target: ClockPoint {
            domain: host_domain,
            timestamp: target,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macos::audio::CoreAudioRequest;
    use beatkernel::{audio::AudioFormat, time::ClockMappingQuality};
    struct Mapping;
    impl ClockMapper for Mapping {
        fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
            if from.domain != ClockDomainId(1) || to != ClockDomainId(3) {
                return None;
            }
            from.timestamp
                .checked_add(beatkernel::time::Duration::from_nanos(10))
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Exact
        }
    }
    fn point(domain: u32, ns: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(domain),
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn config() -> CoreAudioApplied {
        let format = AudioFormat::new(3, 2).unwrap();
        CoreAudioApplied {
            request: CoreAudioRequest {
                device: 42,
                format,
                buffer_frames: 64,
            },
            format,
            buffer_frames: 64,
            buffer_channels: vec![1, 1],
            output_domain: ClockDomainId(2),
            output_origin: Timestamp::from_nanos(-10),
            native_clock: ClockDomainId(1),
        }
    }
    fn observation() -> CoreAudioPresentation {
        CoreAudioPresentation {
            host_ticks: 999,
            native: Some(point(1, 2_000_000_000)),
            native_sample_frame: Some(123.5),
            output_grid: Some(point(2, 333_333_323)),
            flags: 3,
            first_frame: 1,
            frames: 32,
        }
    }
    #[test]
    fn checked_floor_grid_uses_supplied_native_mapper_not_float_or_receipt() {
        let mut value = observation();
        let first = coreaudio_presentation_pair(value, &config(), ClockDomainId(3), &Mapping)
            .unwrap()
            .unwrap();
        assert_eq!(first.source, point(2, 333_333_323));
        assert_eq!(first.target, point(3, 2_000_000_010)); // A future native point is retained.
        value.host_ticks = u64::MAX; // Raw ticks cannot be authenticated by this pure helper.
        value.native_sample_frame = Some(-987654321.25);
        assert_eq!(
            coreaudio_presentation_pair(value, &config(), ClockDomainId(3), &Mapping).unwrap(),
            Some(first)
        );
        value.first_frame = 0;
        value.output_grid = Some(point(2, -10));
        assert_eq!(
            coreaudio_presentation_pair(value, &config(), ClockDomainId(3), &Mapping)
                .unwrap()
                .unwrap()
                .source,
            point(2, -10)
        );
    }
    #[test]
    fn missing_association_returns_none_and_inconsistent_metadata_rejects() {
        let original = observation();
        let mut value = original;
        value.native = None;
        assert_eq!(
            coreaudio_presentation_pair(value, &config(), ClockDomainId(3), &Mapping).unwrap(),
            None
        );
        let mut value = original;
        value.output_grid = None;
        value.first_frame = u64::MAX;
        assert_eq!(
            coreaudio_presentation_pair(value, &config(), ClockDomainId(3), &Mapping).unwrap(),
            None
        );
        let mut value = original;
        value.flags = 1;
        value.native = None;
        assert_eq!(
            coreaudio_presentation_pair(value, &config(), ClockDomainId(3), &Mapping).unwrap(),
            None
        );
        let mut invalid = Vec::new();
        let mut value = original;
        value.frames = 0;
        invalid.push(value);
        let mut value = original;
        value.frames = 65;
        invalid.push(value);
        let mut value = original;
        value.flags = 1;
        invalid.push(value);
        let mut value = original;
        value.flags = 2;
        invalid.push(value);
        let mut value = original;
        value.native_sample_frame = None;
        invalid.push(value);
        let mut value = original;
        value.native_sample_frame = Some(f64::NAN);
        invalid.push(value);
        let mut value = original;
        value.native_sample_frame = Some(f64::INFINITY);
        invalid.push(value);
        let mut value = original;
        value.output_grid = Some(point(2, 333_333_324));
        invalid.push(value);
        for value in invalid {
            assert!(matches!(
                coreaudio_presentation_pair(value, &config(), ClockDomainId(3), &Mapping),
                Err(CoreAudioPresentationError::InvalidObservation(_))
            ));
        }
    }
    #[test]
    fn domain_configuration_and_unmapped_errors_are_explicit() {
        let mut value = observation();
        value.native.as_mut().unwrap().domain = ClockDomainId(9);
        assert!(matches!(
            coreaudio_presentation_pair(value, &config(), ClockDomainId(3), &Mapping),
            Err(CoreAudioPresentationError::DomainMismatch { .. })
        ));
        let mut value = observation();
        value.output_grid.as_mut().unwrap().domain = ClockDomainId(9);
        assert!(matches!(
            coreaudio_presentation_pair(value, &config(), ClockDomainId(3), &Mapping),
            Err(CoreAudioPresentationError::DomainMismatch { .. })
        ));
        assert!(matches!(
            coreaudio_presentation_pair(observation(), &config(), ClockDomainId(9), &Mapping),
            Err(CoreAudioPresentationError::UnmappedHost { .. })
        ));
        let mut invalid = Vec::new();
        let mut applied = config();
        applied.buffer_channels = vec![1; 33];
        invalid.push(applied);
        let mut applied = config();
        applied.request.device = 0;
        invalid.push(applied);
        let mut applied = config();
        applied.buffer_frames = 0;
        invalid.push(applied);
        let mut applied = config();
        applied.buffer_frames = 63;
        invalid.push(applied);
        let mut applied = config();
        applied.format = AudioFormat::new(4, 2).unwrap();
        invalid.push(applied);
        let mut applied = config();
        applied.buffer_channels = vec![2, 1];
        invalid.push(applied);
        let mut applied = config();
        applied.buffer_channels = vec![];
        invalid.push(applied);
        let mut applied = config();
        applied.buffer_channels = vec![0, 2];
        invalid.push(applied);
        for applied in invalid {
            assert!(matches!(
                coreaudio_presentation_pair(observation(), &applied, ClockDomainId(3), &Mapping),
                Err(CoreAudioPresentationError::InvalidConfiguration(_))
            ));
        }
    }
    #[test]
    fn origin_is_added_before_narrowing_and_unrepresentable_grid_rejects() {
        let mut applied = config();
        applied.format = AudioFormat::new(1_000_000_000, 2).unwrap();
        applied.request.format = applied.format;
        applied.output_origin = Timestamp::from_nanos(i64::MIN);
        let mut value = observation();
        value.first_frame = u64::MAX;
        value.output_grid = Some(point(2, i64::MAX));
        assert_eq!(
            coreaudio_presentation_pair(value, &applied, ClockDomainId(3), &Mapping)
                .unwrap()
                .unwrap()
                .source
                .timestamp
                .as_nanos(),
            i64::MAX
        );
        assert!(matches!(
            coreaudio_presentation_pair(value, &config(), ClockDomainId(3), &Mapping),
            Err(CoreAudioPresentationError::Overflow)
        ));
        applied.output_origin = Timestamp::ZERO;
        assert!(matches!(
            coreaudio_presentation_pair(value, &applied, ClockDomainId(3), &Mapping),
            Err(CoreAudioPresentationError::Overflow)
        ));
    }
}
