//! Pure native-status interpretation and separately coherent atomic publication.
use beatkernel::time::{ClockDomainId, ClockMappingQuality, ClockPoint, Timestamp};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU64, Ordering};

/// Raw ALSA `snd_htimestamp_t`, preserving invalid values for diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlsaNativeTimestamp {
    /// Native timespec seconds.
    pub seconds: i64,
    /// Native timespec nanoseconds, valid only within 0..1,000,000,000.
    pub nanoseconds: i64,
}
/// One coherent native-status query, independent of aggregate software counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlsaTimingSnapshot {
    /// Raw ALSA state integer; RUNNING is 3, unknown values are retained.
    pub native_state: i32,
    /// Same-worker frames successfully submitted before this status query.
    pub submitted_frames: u64,
    /// Raw signed native delay in frames.
    pub delay_frames: i64,
    /// Frames available according to the same native status.
    pub available_frames: u64,
    /// Raw native high-resolution timestamp, without receipt substitution.
    pub native_htstamp: AlsaNativeTimestamp,
    /// Checked nonzero native monotonic association, when representable.
    pub native_timestamp: Option<ClockPoint>,
    /// Explicit CLOCK_MONOTONIC call-start observation.
    pub query_started: ClockPoint,
    /// Explicit CLOCK_MONOTONIC call-finish observation, not a native timestamp.
    pub query_finished: ClockPoint,
    /// Checked submitted-minus-delay estimate for native RUNNING only.
    pub estimated_played_frames: Option<u64>,
    /// No acoustic or numeric timing accuracy bound is established.
    pub quality: ClockMappingQuality,
    /// Applied/readback native timestamp mode, ENABLE=1.
    pub timestamp_mode: i32,
    /// Applied/readback native timestamp type, MONOTONIC=1.
    pub timestamp_type: i32,
}
impl AlsaNativeTimestamp {
    fn point(self, domain: ClockDomainId) -> Option<ClockPoint> {
        if self.seconds < 0 || !(0..1_000_000_000).contains(&self.nanoseconds) {
            return None;
        }
        let ns = i128::from(self.seconds)
            .checked_mul(1_000_000_000)?
            .checked_add(i128::from(self.nanoseconds))?;
        if ns == 0 {
            return None;
        }
        Some(ClockPoint {
            domain,
            timestamp: Timestamp::from_nanos(i64::try_from(ns).ok()?),
        })
    }
}
pub(super) fn interpret(
    native_state: i32,
    submitted_frames: u64,
    delay_frames: i64,
    available_frames: u64,
    native_htstamp: AlsaNativeTimestamp,
    query_started: ClockPoint,
    query_finished: ClockPoint,
    timestamp_mode: i32,
    timestamp_type: i32,
) -> AlsaTimingSnapshot {
    let native_timestamp = (timestamp_mode == 1
        && timestamp_type == 1
        && query_started.domain == query_finished.domain
        && query_started.timestamp <= query_finished.timestamp)
        .then(|| native_htstamp.point(query_started.domain))
        .flatten();
    let estimated_played_frames = if native_state == 3
        && native_timestamp.is_some()
        && timestamp_mode == 1
        && timestamp_type == 1
    {
        u64::try_from(delay_frames)
            .ok()
            .and_then(|delay| submitted_frames.checked_sub(delay))
    } else {
        None
    };
    AlsaTimingSnapshot {
        native_state,
        submitted_frames,
        delay_frames,
        available_frames,
        native_htstamp,
        native_timestamp,
        query_started,
        query_finished,
        estimated_played_frames,
        quality: ClockMappingQuality::Unknown,
        timestamp_mode,
        timestamp_type,
    }
}

pub(super) struct TimingShared {
    sequence: AtomicU64,
    valid: AtomicBool,
    state: AtomicI32,
    mode: AtomicI32,
    kind: AtomicI32,
    submitted: AtomicU64,
    delay: AtomicI64,
    available: AtomicU64,
    seconds: AtomicI64,
    nanoseconds: AtomicI64,
    started: AtomicI64,
    finished: AtomicI64,
}
impl TimingShared {
    pub(super) fn new() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            valid: AtomicBool::new(false),
            state: AtomicI32::new(0),
            mode: AtomicI32::new(0),
            kind: AtomicI32::new(0),
            submitted: AtomicU64::new(0),
            delay: AtomicI64::new(0),
            available: AtomicU64::new(0),
            seconds: AtomicI64::new(0),
            nanoseconds: AtomicI64::new(0),
            started: AtomicI64::new(0),
            finished: AtomicI64::new(0),
        }
    }
    // Exactly one writer (worker, or caller only after worker join). All fields
    // are atomic and SeqCst, so a reader cannot accept crossed publications.
    fn begin(&self) -> bool {
        if self.sequence.load(Ordering::SeqCst) >= u64::MAX - 1 {
            self.sequence.store(u64::MAX, Ordering::SeqCst);
            self.valid.store(false, Ordering::SeqCst);
            return false;
        }
        self.sequence.fetch_add(1, Ordering::SeqCst);
        true
    }
    fn end(&self) {
        self.sequence.fetch_add(1, Ordering::SeqCst);
    }
    pub(super) fn invalidate(&self) {
        if self.begin() {
            self.valid.store(false, Ordering::SeqCst);
            self.end();
        }
    }
    pub(super) fn publish(&self, snapshot: AlsaTimingSnapshot) {
        if !self.begin() {
            return;
        }
        self.state.store(snapshot.native_state, Ordering::SeqCst);
        self.mode.store(snapshot.timestamp_mode, Ordering::SeqCst);
        self.kind.store(snapshot.timestamp_type, Ordering::SeqCst);
        self.submitted
            .store(snapshot.submitted_frames, Ordering::SeqCst);
        self.delay.store(snapshot.delay_frames, Ordering::SeqCst);
        self.available
            .store(snapshot.available_frames, Ordering::SeqCst);
        self.seconds
            .store(snapshot.native_htstamp.seconds, Ordering::SeqCst);
        self.nanoseconds
            .store(snapshot.native_htstamp.nanoseconds, Ordering::SeqCst);
        self.started.store(
            snapshot.query_started.timestamp.as_nanos(),
            Ordering::SeqCst,
        );
        self.finished.store(
            snapshot.query_finished.timestamp.as_nanos(),
            Ordering::SeqCst,
        );
        self.valid.store(true, Ordering::SeqCst);
        self.end();
    }
    pub(super) fn snapshot(&self, domain: ClockDomainId) -> Option<AlsaTimingSnapshot> {
        for _ in 0..64 {
            let before = self.sequence.load(Ordering::SeqCst);
            if before & 1 != 0 {
                continue;
            }
            if !self.valid.load(Ordering::SeqCst) {
                return None;
            }
            let snapshot = interpret(
                self.state.load(Ordering::SeqCst),
                self.submitted.load(Ordering::SeqCst),
                self.delay.load(Ordering::SeqCst),
                self.available.load(Ordering::SeqCst),
                AlsaNativeTimestamp {
                    seconds: self.seconds.load(Ordering::SeqCst),
                    nanoseconds: self.nanoseconds.load(Ordering::SeqCst),
                },
                ClockPoint {
                    domain,
                    timestamp: Timestamp::from_nanos(self.started.load(Ordering::SeqCst)),
                },
                ClockPoint {
                    domain,
                    timestamp: Timestamp::from_nanos(self.finished.load(Ordering::SeqCst)),
                },
                self.mode.load(Ordering::SeqCst),
                self.kind.load(Ordering::SeqCst),
            );
            if self.sequence.load(Ordering::SeqCst) == before {
                return Some(snapshot);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn point(ns: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(7),
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn sample(
        state: i32,
        submitted: u64,
        delay: i64,
        stamp: AlsaNativeTimestamp,
    ) -> AlsaTimingSnapshot {
        interpret(
            state,
            submitted,
            delay,
            99,
            stamp,
            point(2_000_000_000),
            point(2_000_000_100),
            1,
            1,
        )
    }
    fn stamp() -> AlsaNativeTimestamp {
        AlsaNativeTimestamp {
            seconds: 1,
            nanoseconds: 123,
        }
    }
    #[test]
    fn native_stamp_before_call_bracket_is_preserved_not_substituted() {
        let value = sample(3, 100, 40, stamp());
        assert_eq!(value.native_timestamp, Some(point(1_000_000_123)));
        assert_eq!(value.estimated_played_frames, Some(60));
        assert_eq!(value.available_frames, 99);
        assert_eq!(value.quality, ClockMappingQuality::Unknown);
        assert_eq!((value.timestamp_mode, value.timestamp_type), (1, 1));
        assert_eq!(value.native_htstamp, stamp());
    }
    #[test]
    fn invalid_zero_and_extreme_timespec_remains_raw_without_estimate() {
        for raw in [
            AlsaNativeTimestamp {
                seconds: 0,
                nanoseconds: 0,
            },
            AlsaNativeTimestamp {
                seconds: -1,
                nanoseconds: 1,
            },
            AlsaNativeTimestamp {
                seconds: 1,
                nanoseconds: -1,
            },
            AlsaNativeTimestamp {
                seconds: 1,
                nanoseconds: 1_000_000_000,
            },
            AlsaNativeTimestamp {
                seconds: i64::MAX,
                nanoseconds: 0,
            },
        ] {
            let value = sample(3, 100, 0, raw);
            assert_eq!(value.native_htstamp, raw);
            assert_eq!(value.native_timestamp, None);
            assert_eq!(value.estimated_played_frames, None);
        }
        let maximum = AlsaNativeTimestamp {
            seconds: i64::MAX / 1_000_000_000,
            nanoseconds: i64::MAX % 1_000_000_000,
        };
        assert_eq!(
            sample(3, 1, 0, maximum)
                .native_timestamp
                .unwrap()
                .timestamp
                .as_nanos(),
            i64::MAX
        );
    }
    #[test]
    fn signed_delay_boundaries_and_unknown_states_do_not_invent_played_frames() {
        assert_eq!(sample(3, 0, 0, stamp()).estimated_played_frames, Some(0));
        assert_eq!(
            sample(3, u64::MAX, i64::MAX, stamp()).estimated_played_frames,
            Some(1u64 << 63)
        );
        for delay in [i64::MIN, -1, 101, i64::MAX] {
            assert_eq!(sample(3, 100, delay, stamp()).estimated_played_frames, None);
        }
        for state in [0, 1, 2, 4, 5, 6, 7, 8, 1024, i32::MAX, -1] {
            let value = sample(state, 100, 20, stamp());
            assert_eq!(value.native_state, state);
            assert!(value.native_timestamp.is_some());
            assert_eq!(value.estimated_played_frames, None);
        }
    }
    #[test]
    fn receipt_metadata_mismatch_or_regression_cannot_authorize_estimate() {
        let mismatch = interpret(
            3,
            10,
            1,
            0,
            stamp(),
            point(1),
            ClockPoint {
                domain: ClockDomainId(8),
                timestamp: Timestamp::from_nanos(2),
            },
            1,
            1,
        );
        assert_eq!(mismatch.native_timestamp, None);
        assert_eq!(mismatch.estimated_played_frames, None);
        assert_eq!(
            interpret(3, 10, 1, 0, stamp(), point(2), point(1), 1, 1).estimated_played_frames,
            None
        );
    }
    #[test]
    fn applied_timestamp_metadata_is_preserved_and_no_fallback_is_inferred() {
        for (mode, kind) in [(0, 1), (1, 0), (1, 2), (99, 99)] {
            let value = interpret(3, 100, 1, 0, stamp(), point(1), point(2), mode, kind);
            assert_eq!((value.timestamp_mode, value.timestamp_type), (mode, kind));
            assert_eq!(value.native_timestamp, None);
            assert_eq!(value.estimated_played_frames, None);
        }
    }
    #[test]
    fn publication_replacement_invalidation_busy_and_exhaustion_are_explicit() {
        let shared = TimingShared::new();
        assert_eq!(shared.snapshot(ClockDomainId(7)), None);
        let first = sample(3, 100, 40, stamp());
        shared.publish(first);
        assert_eq!(shared.snapshot(ClockDomainId(7)), Some(first));
        shared.invalidate();
        assert_eq!(shared.snapshot(ClockDomainId(7)), None);
        let second = sample(2, 200, -1, stamp());
        shared.publish(second);
        assert_eq!(shared.snapshot(ClockDomainId(7)), Some(second));
        shared.sequence.store(1, Ordering::SeqCst);
        assert_eq!(shared.snapshot(ClockDomainId(7)), None);
        shared.sequence.store(u64::MAX - 1, Ordering::SeqCst);
        shared.publish(first);
        assert_eq!(shared.snapshot(ClockDomainId(7)), None);
        assert_eq!(shared.sequence.load(Ordering::SeqCst), u64::MAX);
    }
}
