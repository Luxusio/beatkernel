//! Pure checked ALSA played-frame estimate to explicit output/host association.
use super::{AlsaTimingSnapshot, LinuxError};
use beatkernel::time::{ClockMappingQuality, ClockPair, ClockPoint};

/// Converts native ALSA timing into a caller-domain output/host pair.
///
/// The applied rate must be explicitly supplied. Absent native timestamp/played
/// estimate yields None. Provided public snapshot fields are checked for internal
/// consistency; no userspace call timestamp replaces the native association.
/// The resulting estimate has no acoustic or numeric accuracy bound.
pub fn alsa_presentation_pair(
    snapshot: AlsaTimingSnapshot,
    output_origin: ClockPoint,
    sample_rate: u32,
) -> Result<Option<ClockPair>, LinuxError> {
    let basis =
        beatkernel::audio::OutputFrameBasis::new(output_origin, sample_rate, 0).map_err(|_| {
            LinuxError::InvalidConfiguration(
                "ALSA presentation requires nonzero applied sample rate",
            )
        })?;
    alsa_presentation_pair_with_basis(snapshot, basis)
}
/// Maps verified native played frames through the original physical mixer grid.
/// Native counters remain stream-relative; the frame offset is added before floor.
pub fn alsa_presentation_pair_with_basis(
    snapshot: AlsaTimingSnapshot,
    basis: beatkernel::audio::OutputFrameBasis,
) -> Result<Option<ClockPair>, LinuxError> {
    let Some((played, native)) = validated_native_pair(snapshot)? else {
        return Ok(None);
    };
    let output = basis
        .point_at_stream_frame(played)
        .map_err(|_| LinuxError::Overflow)?;
    Ok(Some(ClockPair {
        source: output,
        target: native,
    }))
}

/// Maps actual native target frames from an exact piecewise-rate physical start.
pub fn alsa_presentation_pair_with_target_basis(
    snapshot: AlsaTimingSnapshot,
    basis: beatkernel::audio::TargetFrameBasis,
) -> Result<Option<ClockPair>, LinuxError> {
    let Some((played, native)) = validated_native_pair(snapshot)? else {
        return Ok(None);
    };
    let output = basis
        .point_at_stream_frame(played)
        .map_err(|_| LinuxError::Overflow)?;
    Ok(Some(ClockPair {
        source: output,
        target: native,
    }))
}
fn validated_native_pair(
    snapshot: AlsaTimingSnapshot,
) -> Result<Option<(u64, ClockPoint)>, LinuxError> {
    let (Some(played), Some(native)) =
        (snapshot.estimated_played_frames, snapshot.native_timestamp)
    else {
        return Ok(None);
    };
    if snapshot.native_state != 3
        || snapshot.timestamp_mode != 1
        || snapshot.timestamp_type != 1
        || snapshot.quality != ClockMappingQuality::Unknown
    {
        return Err(LinuxError::InvalidConfiguration(
            "ALSA estimate requires native RUNNING, ENABLE/MONOTONIC and Unknown accuracy",
        ));
    }
    if native.domain != snapshot.query_started.domain
        || native.domain != snapshot.query_finished.domain
        || snapshot.query_started.timestamp > snapshot.query_finished.timestamp
    {
        return Err(LinuxError::InvalidConfiguration(
            "ALSA native timestamp/query domain or chronology mismatch",
        ));
    }
    let raw = snapshot.native_htstamp;
    if raw.seconds < 0 || !(0..1_000_000_000).contains(&raw.nanoseconds) {
        return Err(LinuxError::InvalidConfiguration(
            "ALSA native timespec is invalid",
        ));
    }
    let stamp = i128::from(raw.seconds)
        .checked_mul(1_000_000_000)
        .and_then(|seconds| seconds.checked_add(i128::from(raw.nanoseconds)))
        .ok_or(LinuxError::Overflow)?;
    let stamp = i64::try_from(stamp).map_err(|_| LinuxError::Overflow)?;
    if stamp == 0 || native.timestamp.as_nanos() != stamp {
        return Err(LinuxError::InvalidConfiguration(
            "ALSA native timestamp does not match nonzero raw timespec",
        ));
    }
    let expected = u64::try_from(snapshot.delay_frames)
        .ok()
        .and_then(|delay| snapshot.submitted_frames.checked_sub(delay));
    if expected != Some(played) {
        return Err(LinuxError::InvalidConfiguration(
            "ALSA played estimate contradicts submitted-minus-delay",
        ));
    }
    Ok(Some((played, native)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux::AlsaNativeTimestamp;
    use beatkernel::time::{ClockDomainId, Timestamp};
    fn point(domain: u32, ns: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(domain),
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn snapshot(played: u64) -> AlsaTimingSnapshot {
        AlsaTimingSnapshot {
            native_state: 3,
            submitted_frames: played.checked_add(7).unwrap(),
            delay_frames: 7,
            available_frames: 99,
            native_htstamp: AlsaNativeTimestamp {
                seconds: 1,
                nanoseconds: 123,
            },
            native_timestamp: Some(point(1, 1_000_000_123)),
            query_started: point(1, 2_000_000_000),
            query_finished: point(1, 2_000_000_100),
            estimated_played_frames: Some(played),
            quality: ClockMappingQuality::Unknown,
            timestamp_mode: 1,
            timestamp_type: 1,
        }
    }
    #[test]
    fn native_association_and_floor_frame_grid_are_explicit_including_zero() {
        let value = snapshot(1);
        let pair = alsa_presentation_pair(value, point(2, -10), 3)
            .unwrap()
            .unwrap();
        assert_eq!(pair.source, point(2, 333_333_323));
        assert_eq!(pair.target, value.native_timestamp.unwrap());
        assert!(pair.target.timestamp < value.query_started.timestamp);
        assert_eq!(
            alsa_presentation_pair(snapshot(0), point(2, -10), 48_000)
                .unwrap()
                .unwrap()
                .source,
            point(2, -10)
        );
        assert!(matches!(
            alsa_presentation_pair(value, point(2, 0), 0),
            Err(LinuxError::InvalidConfiguration(_))
        ));
    }
    #[test]
    fn absence_is_unavailable_and_contradictory_public_metadata_rejects() {
        let original = snapshot(10);
        let mut missing = original;
        missing.estimated_played_frames = None;
        assert_eq!(
            alsa_presentation_pair(missing, point(2, 0), 48_000).unwrap(),
            None
        );
        missing = original;
        missing.native_timestamp = None;
        assert_eq!(
            alsa_presentation_pair(missing, point(2, 0), 48_000).unwrap(),
            None
        );
        let mut invalid = Vec::new();
        let mut value = original;
        value.native_state = 2;
        invalid.push(value);
        let mut value = original;
        value.timestamp_type = 0;
        invalid.push(value);
        let mut value = original;
        value.timestamp_mode = 0;
        invalid.push(value);
        let mut value = original;
        value.quality = ClockMappingQuality::Exact;
        invalid.push(value);
        let mut value = original;
        value.delay_frames = -1;
        invalid.push(value);
        let mut value = original;
        value.delay_frames = 18;
        invalid.push(value);
        let mut value = original;
        value.estimated_played_frames = Some(11);
        invalid.push(value);
        let mut value = original;
        value.native_htstamp.nanoseconds = -1;
        invalid.push(value);
        let mut value = original;
        value.native_htstamp.nanoseconds = 1_000_000_000;
        invalid.push(value);
        let mut value = original;
        value.native_htstamp = AlsaNativeTimestamp {
            seconds: 0,
            nanoseconds: 0,
        };
        value.native_timestamp = Some(point(1, 0));
        invalid.push(value);
        let mut value = original;
        value.native_timestamp = Some(point(1, 1_000_000_124));
        invalid.push(value);
        let mut value = original;
        value.query_finished.domain = ClockDomainId(3);
        invalid.push(value);
        let mut value = original;
        value.query_finished.timestamp = Timestamp::ZERO;
        invalid.push(value);
        for value in invalid {
            assert!(matches!(
                alsa_presentation_pair(value, point(2, 0), 48_000),
                Err(LinuxError::InvalidConfiguration(_))
            ));
        }
    }
    #[test]
    fn checked_large_frame_grid_adds_origin_before_narrowing() {
        let mut value = snapshot(0);
        value.submitted_frames = u64::MAX;
        value.delay_frames = 0;
        value.estimated_played_frames = Some(u64::MAX);
        assert_eq!(
            alsa_presentation_pair(value, point(2, i64::MIN), 1_000_000_000)
                .unwrap()
                .unwrap()
                .source
                .timestamp
                .as_nanos(),
            i64::MAX
        );
        assert!(matches!(
            alsa_presentation_pair(value, point(2, 0), 1),
            Err(LinuxError::Overflow)
        ));
        assert!(matches!(
            alsa_presentation_pair(snapshot(1), point(2, i64::MAX), 48_000),
            Err(LinuxError::Overflow)
        ));
        value.native_htstamp.seconds = i64::MAX;
        assert!(matches!(
            alsa_presentation_pair(value, point(2, 0), 48_000),
            Err(LinuxError::Overflow)
        ));
    }
}
